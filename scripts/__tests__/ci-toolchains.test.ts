import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { isolateBuildToolchain, unusedToolchains } from '../ci/isolate-build-toolchain.mjs'

vi.mock('node:child_process', () => {
  const mock = { execFileSync: vi.fn() }
  return { ...mock, default: mock }
})
beforeEach(() => vi.clearAllMocks())

const pinned = '1.95.0-x86_64-pc-windows-msvc'
const floating = 'stable-x86_64-pc-windows-msvc'

describe('hosted build toolchain isolation', () => {
  it('keeps the active pinned compiler and removes unused floating toolchains', () => {
    expect(unusedToolchains([floating, pinned], pinned, '1.95.0')).toEqual([floating])
  })

  it('does not remove compilers if the active toolchain disagrees with the pin', () => {
    expect(() => unusedToolchains([floating, pinned], floating, '1.95.0')).toThrow(
      'active toolchain'
    )
    expect(() => unusedToolchains([floating], pinned, '1.95.0')).toThrow('not installed')
  })

  it.each([{}, { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'self-hosted' }])(
    'refuses to change a local or self-hosted Rust installation',
    env => {
      expect(() => isolateBuildToolchain('1.95.0', env)).toThrow('GitHub-hosted')
      expect(execFileSync).not.toHaveBeenCalled()
    }
  )

  it('runs uninstall only for the unused installed compiler', () => {
    const exec = vi.mocked(execFileSync)
    exec
      .mockReturnValueOnce(`${pinned} (directory override)\n`)
      .mockReturnValueOnce(`${floating}\n${pinned}\n`)
      .mockReturnValueOnce('')
      .mockReturnValueOnce(`${pinned}\n`)
    isolateBuildToolchain('1.95.0', { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' })
    expect(exec.mock.calls[2]?.slice(0, 2)).toEqual([
      'rustup',
      ['toolchain', 'uninstall', floating],
    ])
  })

  it('isolates both desktop and CLI builds before cache lookup', () => {
    for (const file of ['build.yml', 'build-cli.yml']) {
      const source = fs.readFileSync(
        path.resolve(__dirname, '../../.github/workflows', file),
        'utf8'
      )
      const isolation = source.indexOf('run: node scripts/ci/isolate-build-toolchain.mjs')
      expect(isolation).toBeGreaterThan(source.indexOf('name: install Rust toolchain'))
      expect(isolation).toBeLessThan(source.indexOf('name: Cache Rust dependencies'))
    }
  })
})
