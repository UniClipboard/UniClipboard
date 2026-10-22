import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'

type Step = {
  uses?: string
  with?: { operation?: string; scene?: string; 'object-fallback'?: string }
}
type Job = {
  steps?: Step[]
  env?: { MBX_CACHE_EXPORT_GROUP?: string }
  if?: string
  environment?: string
}
const yaml = (file: string) => parse(fs.readFileSync(file, 'utf8'))

describe('MBX request reduction', () => {
  it('enforces snapshot boundaries and summarizes logical requests', () => {
    execFileSync('python3', ['scripts/ci/mbx-snapshot.test.py'], { stdio: 'pipe' })
  })

  it('cancels outdated runs independently for each PR', () => {
    expect(yaml('.github/workflows/pr-check.yml').concurrency).toEqual({
      group: 'pr-check-${{ github.event.pull_request.number }}',
      'cancel-in-progress': true,
    })
  })

  it('uses the same Rust feature graphs for the reader, writer and comparison', () => {
    for (const file of ['pr-check.yml', 'cache-warmup.yml', 'mbx-cache-benchmark.yml']) {
      const jobs = yaml(`.github/workflows/${file}`).jobs as Record<string, Job>
      const builds = Object.values(jobs).filter(job =>
        job.steps?.some(step => step.uses === './.github/actions/build-e2e-rust')
      )
      expect(builds).toHaveLength(1)
      expect(builds[0].env?.MBX_CACHE_EXPORT_GROUP).toContain('e2e-macos')
    }
  })

  it('uses matching platform workloads for PR readers, warmup and comparisons', () => {
    const pr = yaml('.github/workflows/pr-check.yml').jobs
    const warm = yaml('.github/workflows/cache-warmup.yml').jobs['warmup-pr-rust']
    const compare = yaml('.github/workflows/mbx-cache-benchmark.yml').jobs.compare
    expect(warm.strategy.matrix.include.map((entry: { runner: string }) => entry.runner)).toEqual([
      'ubuntu-latest',
      'windows-latest',
      'windows-11-arm',
    ])
    expect(warm.if).toContain('github.ref_protected')
    expect(warm.if).toContain("github.ref == 'refs/heads/main'")
    expect(warm.environment).toBe('desktop-build-cache-writer')
    for (const [job, scene] of [
      [pr.rust, 'pr-check-linux'],
      [pr['quick-panel-windows'], 'quick-panel-windows'],
    ] as const) {
      expect(job.env.MBX_CACHE_EXPORT_GROUP).toBe(scene)
      expect(job.environment).toBe('desktop-build-cache-reader')
      const restore = job.steps.find((step: Step) => step.uses === './.github/actions/mbx-snapshot')
      expect(restore.with).toEqual({ scene, 'object-fallback': 'false' })
      expect(job.steps.some((step: Step) => step.with?.operation === 'save')).toBe(false)
      expect(
        job.steps.find((step: Step) => step.uses === './.github/actions/build-pr-rust').with.scene
      ).toBe(scene)
    }
    for (const job of [warm, compare]) {
      expect(
        job.steps.find((step: Step) => step.uses === './.github/actions/build-pr-rust').with.scene
      ).toContain(job === warm ? 'matrix.scene' : 'MBX_CACHE_EXPORT_GROUP')
    }
    expect(warm.strategy.matrix.include[0].container).toBe(pr.rust.container.image)
    expect(
      yaml('.github/workflows/mbx-snapshot-validation.yml').jobs['native-roundtrip'].strategy.matrix
        .runner
    ).toEqual(['ubuntu-latest', 'macos-latest', 'windows-latest', 'windows-11-arm'])
  })

  it('restricts snapshot publication to protected main and keeps PRs read-only', () => {
    const writer = yaml('.github/workflows/cache-warmup.yml').jobs['warmup-e2e-macos']
    expect(writer.if).toContain("github.ref == 'refs/heads/main'")
    expect(writer.if).toContain('github.ref_protected')
    expect(writer.environment).toBe('desktop-build-cache-writer')
    const reader = yaml('.github/workflows/pr-check.yml').jobs.e2e
    expect(reader.environment).toBe('desktop-build-cache-reader')
    expect(reader.steps.some((step: Step) => step.with?.operation === 'save')).toBe(false)
    expect(
      reader.steps.find((step: Step) => step.uses === './.github/actions/mbx-snapshot').with[
        'object-fallback'
      ]
    ).toBe('false')
  })
})
