import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'

const root = path.resolve(import.meta.dirname, '../..')
const read = (file: string) => parse(fs.readFileSync(path.join(root, file), 'utf8'))
const action = read('.github/actions/go-host-apt/action.yml')
const pr = read('.github/workflows/pr-check.yml')
const warmup = read('.github/workflows/cache-warmup.yml')

describe('Go host apt download cache', () => {
  it('checks cold/hot archives, corruption rejection and key invalidation', () => {
    execFileSync('python3', ['scripts/ci/go-host-apt.test.py'], { cwd: root })
  })

  it('shares the Ubuntu 24.04 package resolution between consumer and producer', () => {
    expect(pr.jobs['gui-go']['runs-on']).toBe('ubuntu-24.04')
    expect(warmup.jobs['warmup-go-host-apt']['runs-on']).toBe('ubuntu-24.04')
    expect(pr.jobs['gui-go'].steps).toContainEqual({
      name: 'Install GTK4 build dependencies',
      uses: './.github/actions/go-host-apt',
    })
    expect(warmup.jobs['warmup-go-host-apt'].steps).toContainEqual({
      uses: './.github/actions/go-host-apt',
      with: { 'download-only': 'true', save: 'true' },
    })
    expect(pr.jobs.rust.container.image).toBe('ghcr.io/uniclipboard/build-bookworm:latest')
  })

  it('restores exact deb downloads only and restricts writes to protected main', () => {
    const restore = action.runs.steps.find((step: { id?: string }) => step.id === 'cache')
    expect(restore.uses).toBe('actions/cache/restore@v4')
    expect(restore.with.path).toBe('${{ steps.plan.outputs.archives }}/*.deb')
    expect(restore.with['restore-keys']).toBeUndefined()
    expect(action.inputs.save.default).toBe('false')
    const save = action.runs.steps.find(
      (step: { name?: string }) => step.name === 'Save apt downloads'
    )
    expect(save.if).toContain("inputs.save == 'true'")
    expect(save.if).toContain("github.ref == 'refs/heads/main' && github.ref_protected")
    expect(save.if).not.toContain('pull_request')
    expect(save.with).toEqual(restore.with)
    expect(warmup.jobs['warmup-go-host-apt'].if).toBe(
      "github.ref == 'refs/heads/main' && github.ref_protected"
    )
  })
})
