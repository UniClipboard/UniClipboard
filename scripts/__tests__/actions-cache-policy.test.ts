import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { maintainCaches, planCacheCleanup } from '../ci/maintain-actions-cache.mjs'

vi.mock('node:child_process', () => {
  const mock = { execFileSync: vi.fn() }
  return { ...mock, default: mock }
})
beforeEach(() => vi.clearAllMocks())

const GiB = 1024 ** 3
const cache = (id: number, key: string, ref = 'refs/heads/main', size = GiB) => ({
  id,
  key,
  ref,
  size_in_bytes: size,
  created_at: `2026-09-0${id}T00:00:00Z`,
  last_accessed_at: `2026-09-0${id}T00:00:00Z`,
})
const windows = 'v0-rust-x86_64-pc-windows-msvc-Windows_NT-x64-5bb3579f-11111111'
const coverage = 'v0-rust-coverage-Linux-x64-20a16cf1-11111111'

describe('Actions cache retention', () => {
  it('removes legacy PR coverage writes but preserves open-PR CodeQL caches', () => {
    const entries = [
      cache(1, coverage, 'refs/pull/1643/merge'),
      cache(2, `codeql-dependencies-1-Linux-java-${'a'.repeat(64)}`, 'refs/pull/1643/head'),
    ]
    const result = planCacheCleanup(entries, [1643])
    expect(result.remove.map(entry => entry.id)).toEqual([1])
  })

  it('removes closed-PR caches and keeps only the newest generation per ref and family', () => {
    const entries = [
      cache(1, windows),
      cache(2, windows.replace('11111111', '22222222')),
      cache(3, `Linux-bun-${'a'.repeat(64)}`, 'refs/pull/12/merge'),
    ]
    expect(planCacheCleanup(entries, []).remove.map(entry => entry.id)).toEqual([3, 1])
  })

  it('reserves capacity while protecting the default-branch Windows app cache', () => {
    const entries = [
      cache(1, windows, undefined, 2 * GiB),
      cache(2, coverage, undefined, 3 * GiB),
      cache(3, 'v0-rust-pr-check-Linux-x64-11111111-11111111', undefined, 3 * GiB),
      cache(4, `Linux-bun-${'a'.repeat(64)}`, undefined, GiB),
    ]
    const result = planCacheCleanup(entries, [])
    expect(result.remove.map(entry => entry.id)).toEqual([2])
    expect(result.remainingBytes).toBe(6 * GiB)
  })

  it('keeps independent platforms and compiler environments when under budget', () => {
    const entries = [
      cache(1, windows),
      cache(2, windows.replace('5bb3579f', 'aaaaaaaa')),
      cache(3, windows.replace('x86_64-pc', 'aarch64-pc')),
    ]
    expect(planCacheCleanup(entries, []).remove).toEqual([])
  })

  it('does not delete unknown cache families even when they consume the budget', () => {
    const result = planCacheCleanup([cache(1, 'unrelated-service-cache', undefined, 9 * GiB)], [])
    expect(result.remove).toEqual([])
    expect(result.overBudget).toBe(true)
  })
})

describe('cache workflow ownership', () => {
  const root = path.resolve(__dirname, '../..')
  it('cleans after PR and security jobs using only trusted default-branch code', () => {
    const source = fs.readFileSync(path.join(root, '.github/workflows/cache-maintenance.yml'), 'utf8')
    expect(source).toContain("'CodeQL - Code Quality'")
    expect(source).toContain("'PR Check'")
    expect(source).toContain('ref: ${{ github.event.repository.default_branch }}')
    expect(source).not.toContain('workflow_run.head_sha')
    expect(source).not.toContain('workflow_run.head_branch')
  })
  it('disables the implicit cache in both coverage entry points', () => {
    for (const file of ['coverage.yml', 'cache-warmup.yml']) {
      const source = fs.readFileSync(path.join(root, '.github/workflows', file), 'utf8')
      expect(source).toMatch(
        /uses: actions-rust-lang\/setup-rust-toolchain@v1\.15\.2\s+with:\s+cache: false/
      )
    }
  })

  it('warms Windows x64 rather than filling the quota with every release platform', () => {
    const source = fs.readFileSync(path.join(root, '.github/workflows/cache-warmup.yml'), 'utf8')
    expect(source).toContain("platform: 'windows-x86_64'")
    expect(source).not.toContain("platform: 'all'")
  })
})

describe('cache maintenance execution', () => {
  const command = vi.mocked(execFileSync)

  function respondWithOldPullCache() {
    command
      .mockReturnValueOnce(JSON.stringify({ default_branch: 'main' }))
      .mockReturnValueOnce(
        JSON.stringify([{ actions_caches: [cache(1, coverage, 'refs/pull/12/merge')] }])
      )
      .mockReturnValueOnce(JSON.stringify([[]]))
  }

  it('only reads GitHub data by default', () => {
    respondWithOldPullCache()
    const result = maintainCaches({ repository: 'UniClipboard/UniClipboard' })
    expect(result.remove).toHaveLength(1)
    expect(command).toHaveBeenCalledTimes(3)
    expect(command.mock.calls.flatMap(call => call[1] ?? [])).not.toContain('DELETE')
  })

  it('deletes only the immutable IDs in the plan when apply is enabled', () => {
    respondWithOldPullCache()
    command.mockReturnValueOnce('')
    maintainCaches({ repository: 'UniClipboard/UniClipboard', apply: true })
    expect(command).toHaveBeenLastCalledWith(
      'gh',
      ['api', '--method', 'DELETE', 'repos/UniClipboard/UniClipboard/actions/caches/1'],
      { encoding: 'utf8' }
    )
  })

  it('does not delete anything if discovery fails', () => {
    command.mockImplementationOnce(() => {
      throw new Error('API unavailable')
    })
    expect(() => maintainCaches({ repository: 'UniClipboard/UniClipboard', apply: true })).toThrow(
      'API unavailable'
    )
    expect(command).toHaveBeenCalledTimes(1)
  })
})
