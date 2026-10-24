import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'

type Step = {
  uses?: string
  name?: string
  with?: {
    operation?: string
    scene?: string
    'publish-snapshot'?: string
  }
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
    // The Python suite creates an isolated venv and exercises real shell/Git processes.
    execFileSync('python3', ['scripts/ci/mbx-snapshot.test.py'], {
      stdio: 'pipe',
      timeout: 25_000,
    })
  }, 30_000)

  it('skips only unchanged dependency baselines in real Git histories', () => {
    execFileSync('python3', ['scripts/ci/cache-warmup-needed.test.py'], { stdio: 'pipe' })
  })

  it('keeps musl and container snapshot writers off release consumers', () => {
    const cli = yaml('.github/workflows/build-cli.yml').jobs['build-sidecar']
    const restore = cli.steps.find(
      (s: { name: string }) => s.name === 'Restore musl daemon CAS snapshot'
    )
    const save = cli.steps.find((s: { name: string }) => s.name === 'Save musl daemon CAS snapshot')
    expect(restore.run).toContain('--target-args=')
    expect(save.if).toContain('inputs.build_only')
    expect(save.if).toContain('github.ref_protected')
    const linux = yaml('.github/workflows/package-linux-gui.yml').jobs.package
    expect(linux.env.UC_MBX_PUBLISH_SNAPSHOT).toContain('github.ref_protected')
    expect(linux.env.UC_MBX_PUBLISH_SNAPSHOT).toContain('inputs.build_only')
    expect(
      linux.steps.find((s: { name: string }) => s.name === 'Start shared Engine MBX cache').env
        .BUILD_CACHE_ACCESS
    ).toBe("${{ env.UC_MBX_PUBLISH_SNAPSHOT == 'true' && 'write' || 'read' }}")
    for (const name of [
      'Package',
      'Verify the package set',
      'Upload the named release candidates',
    ]) {
      expect(linux.steps.find((s: { name: string }) => s.name === name).if).toBe(
        'inputs.build_only != true'
      )
    }
  })

  it('shares instrumented tests while only protected-main warmup publishes coverage', () => {
    const reader = yaml('.github/workflows/coverage.yml').jobs.coverage
    const writer = yaml('.github/workflows/cache-warmup.yml').jobs['warmup-coverage-rust']
    const call = (job: Job) =>
      job.steps?.find(step => step.uses === './.github/actions/rust-coverage')
    expect(call(reader)?.with?.['publish-snapshot']).toBeUndefined()
    expect(call(writer)?.with?.['publish-snapshot']).toBe('true')
    expect(reader.environment).toBe('desktop-build-cache-reader')
    const steps = yaml('.github/actions/rust-coverage/action.yml').runs.steps
    const position = (name: string) =>
      steps.findIndex((step: { name: string }) => step.name === name)
    expect(position('Restore coverage CAS snapshot')).toBeLessThan(
      position('Generate coverage report')
    )
    expect(position('Generate coverage report')).toBeLessThan(
      position('Save coverage CAS snapshot')
    )
    const save = steps[position('Save coverage CAS snapshot')]
    expect(save.if).toContain('github.ref_protected')
    expect(save.if).toContain("github.ref == 'refs/heads/main'")
    expect(save.if).toContain("inputs.publish-snapshot == 'true'")
    const build = steps[position('Generate coverage report')]
    expect(build.env.MBX_CACHE_EXPORT_GROUP).toBe('coverage-linux')
    expect(build.run).toBe('xvfb-run -a cargo llvm-cov --lcov --output-path lcov.info --workspace')
  })

  it('cancels outdated runs independently for each PR', () => {
    expect(yaml('.github/workflows/pr-check.yml').concurrency).toEqual({
      group: 'pr-check-${{ github.event.pull_request.number }}',
      'cancel-in-progress': true,
    })
  })

  it('uses the same Rust feature graphs for the reader and writer', () => {
    for (const file of ['pr-check.yml', 'cache-warmup.yml']) {
      const jobs = yaml(`.github/workflows/${file}`).jobs as Record<string, Job>
      const builds = Object.values(jobs).filter(job =>
        job.steps?.some(step => step.uses === './.github/actions/build-e2e-rust')
      )
      expect(builds).toHaveLength(1)
      expect(builds[0].env?.MBX_CACHE_EXPORT_GROUP).toContain('e2e-macos')
    }
  })

  it('uses matching platform workloads for PR readers and warmup', () => {
    const pr = yaml('.github/workflows/pr-check.yml').jobs
    const warm = yaml('.github/workflows/cache-warmup.yml').jobs['warmup-pr-rust']
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
      expect(restore.with).toEqual({ scene })
      expect(job.steps.some((step: Step) => step.with?.operation === 'save')).toBe(false)
      expect(
        job.steps.find((step: Step) => step.uses === './.github/actions/build-pr-rust').with.scene
      ).toBe(scene)
    }
    expect(
      warm.steps.find((step: Step) => step.uses === './.github/actions/build-pr-rust').with.scene
    ).toContain('matrix.scene')
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
  })

  it('preserves release source provenance and limits publishers to protected-main warmup', () => {
    const job = yaml('.github/workflows/build-target.yml').jobs['build-component']
    const steps = job.steps
    const pin = steps.findIndex(
      (step: { name?: string }) => step.name === 'Pin snapshot transport to the workflow revision'
    )
    const checkout = steps.findIndex(
      (step: { uses?: string }) => step.uses === 'actions/checkout@v6'
    )
    const restore = steps.findIndex(
      (step: { name?: string }) => step.name === 'Restore release sidecar CAS snapshot'
    )
    const build = steps.findIndex(
      (step: { name?: string }) => step.name === 'prepare uniclipd sidecar'
    )
    const save = steps.findIndex(
      (step: { name?: string }) => step.name === 'Save release sidecar CAS snapshot'
    )
    const provenance = steps.findIndex(
      (step: { name?: string }) => step.name === 'record sidecar provenance'
    )
    expect(pin).toBeLessThan(checkout)
    expect(checkout).toBeLessThan(restore)
    expect(restore).toBeLessThan(build)
    expect(build).toBeLessThan(save)
    expect(save).toBeLessThan(provenance)
    expect(steps[pin].run).toContain('$RUNNER_TEMP/mbx-snapshot-tools.')
    expect(steps[pin].run).toContain('$GITHUB_REPOSITORY/$GITHUB_SHA/scripts/ci/')
    expect(steps[pin]['continue-on-error']).toBe(true)
    expect(steps[checkout].with.ref).toBe('${{ inputs.branch }}')
    for (const index of [restore, save]) {
      expect(steps[index]['continue-on-error']).toBe(true)
      expect(steps[index].env.UC_MBX_SNAPSHOT_SOURCE_ROOT).toBe('${{ github.workspace }}')
      expect(steps[index].run).toContain('$UC_MBX_SNAPSHOT_TOOLS/mbx-snapshot.sh')
      expect(steps[index].run).toContain('--scene release-sidecar')
    }
    for (const key of [
      'SNAPSHOT_TARGET',
      'SNAPSHOT_COMPONENT',
      'SNAPSHOT_BUILD_MODE',
      'SNAPSHOT_TARGET_ARGS',
    ]) {
      expect(steps[restore].env[key]).toBe(steps[save].env[key])
    }
    const publisher = steps[save].if
    for (const guard of [
      'inputs.build_only',
      'inputs.save_cache',
      "github.ref == 'refs/heads/main'",
      'github.ref_protected',
      'inputs.branch == github.sha',
    ]) {
      expect(publisher).toContain(guard)
    }
    expect(yaml('.github/workflows/cache-warmup.yml').jobs['warmup-release-app'].uses).toBe(
      './.github/workflows/build.yml'
    )
    expect(yaml('.github/workflows/build.yml').jobs['build-platform'].uses).toBe(
      './.github/workflows/build-target.yml'
    )
  })

  it('forwards only snapshot secrets through the nested Linux GUI consumer route', () => {
    const names = [
      'BUILD_CACHE_R2_ENDPOINT',
      'BUILD_CACHE_R2_ACCESS_KEY_ID',
      'BUILD_CACHE_R2_SECRET_ACCESS_KEY',
    ]
    const caller = yaml('.github/workflows/build-target.yml').jobs['package-linux-gui']
    const workflow = yaml('.github/workflows/package-linux-gui.yml')
    expect(caller.uses).toBe('./.github/workflows/package-linux-gui.yml')
    expect(caller.secrets).toEqual(
      Object.fromEntries(names.map(name => [name, '${{ secrets.' + name + ' }}']))
    )
    expect(workflow.on.workflow_call.secrets).toEqual(
      Object.fromEntries(names.map(name => [name, { required: false }]))
    )
    expect(caller.if).toBe("inputs.build_only != true && inputs.platform == 'linux-gui'")
    expect(caller.with.build_only).toBeUndefined()
    expect(workflow.on.workflow_call.inputs.build_only.default).toBe(false)
    expect(workflow.jobs.package.environment).toBe(
      "${{ inputs.build_only && inputs.save_cache && github.ref == 'refs/heads/main' && github.ref_protected && 'desktop-build-cache-writer' || 'desktop-build-cache-reader' }}"
    )
    const setup = workflow.jobs.package.steps.find(
      (step: { name: string }) => step.name === 'Start shared Engine MBX cache'
    )
    for (const name of names) expect(setup.env[name]).toBe('${{ secrets.' + name + ' }}')
    // Earlier links already forward these names; do not broaden the final link
    // to unrelated signing, release, or telemetry secrets.
    expect(yaml('.github/workflows/build.yml').jobs['build-platform'].secrets).toBe('inherit')
    expect(yaml('.github/workflows/cache-warmup.yml').jobs['warmup-linux-gui'].secrets).toBe(
      'inherit'
    )
  })

  it('does not leave empty action inputs after removing fallback', () => {
    for (const directory of ['.github/workflows', '.github/actions']) {
      for (const file of fs.readdirSync(directory, { recursive: true }) as string[]) {
        if (!file.endsWith('.yml')) continue
        const document = yaml(`${directory}/${file}`)
        const steps =
          document.runs?.steps ??
          (Object.values(document.jobs ?? {}) as Job[]).flatMap(job => job.steps ?? [])
        for (const step of steps) {
          if ('with' in step) expect(step.with, `${file}: ${step.name ?? step.uses}`).not.toBeNull()
        }
      }
    }
  })

  it('applies local compilation before snapshot transport across CI routes', () => {
    const setup = fs.readFileSync('scripts/build-cache/engine-tools.sh', 'utf8')
    expect(setup).toContain('ci-local-policy.sh')
    expect(setup).toContain('UC_MBX_SNAPSHOT_ENV_FILE=%s')
    const policy = fs.readFileSync('scripts/build-cache/ci-local-policy.sh', 'utf8')
    expect(policy).toContain('${!MBX_REMOTE_@}')
    expect(policy).not.toMatch(/^source /m)
    expect(fs.readFileSync('scripts/build-cache/mbx.sh', 'utf8')).toContain('${GITHUB_ACTIONS:-}')
    const action = yaml('.github/actions/engine-build-cache/action.yml')
    expect(action.runs.steps[0].run).toBe('bash scripts/build-cache/engine-tools.sh --ci')
    for (const file of [
      'pr-check.yml',
      'build-target.yml',
      'build-cli.yml',
      'cache-warmup.yml',
      'membership-e2e.yml',
      'legacy-file-key-store-e2e.yml',
    ]) {
      const jobs = Object.values(yaml(`.github/workflows/${file}`).jobs) as Job[]
      expect(
        jobs.some(job =>
          job.steps?.some(step => step.uses === './.github/actions/engine-build-cache')
        )
      ).toBe(true)
    }
    const linux = yaml('.github/workflows/package-linux-gui.yml').jobs.package
    expect(linux.environment).toContain('desktop-build-cache-reader')
    expect(linux.environment).toContain('github.ref_protected')
    expect(
      linux.steps.find((step: { name: string }) => step.name === 'Start shared Engine MBX cache')
        .run
    ).toBe('bash scripts/build-cache/engine-tools.sh --ci')
    const container = fs.readFileSync('apps/gui-go/e2e/linux/run.sh', 'utf8')
    expect(container).toContain(
      'src=$UC_MBX_SNAPSHOT_ENV_FILE,dst=$UC_MBX_SNAPSHOT_ENV_FILE,readonly'
    )
    expect(container).toContain('-e UC_MBX_SNAPSHOT_ENV_FILE')
    expect(yaml('.github/actions/mbx-snapshot/action.yml').inputs).not.toHaveProperty(
      'object-fallback'
    )
    const warm = yaml('.github/workflows/cache-warmup.yml')
    expect(warm.on.push.paths).toContain('scripts/build-cache/**')
    expect(warm.jobs['warmup-release-app'].with.build_only).toBe(true)
    expect(warm.jobs['warmup-musl-cli'].with.build_only).toBe(true)
    expect(warm.jobs['warmup-linux-gui'].with.build_only).toBe(true)
  })

  it('retains one restore and independent snapshot publication per warmup', () => {
    const jobs = yaml('.github/workflows/cache-warmup.yml').jobs
    for (const job of [jobs['warmup-pr-rust'], jobs['warmup-e2e-macos']]) {
      const restores = job.steps.filter(
        (step: Step) =>
          step.uses === './.github/actions/mbx-snapshot' && step.with?.operation !== 'save'
      )
      expect(restores).toHaveLength(1)
      expect(job.steps.some((step: Step) => step.with?.operation === 'save')).toBe(true)
    }
  })
})
