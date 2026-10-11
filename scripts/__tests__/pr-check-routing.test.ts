import fs from 'node:fs'
import path from 'node:path'
import picomatch from 'picomatch'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'

const root = path.resolve(import.meta.dirname, '../..')
const read = (file: string) => fs.readFileSync(path.join(root, file), 'utf8')
const workflow = (file: string) => parse(read(`.github/workflows/${file}`))

function route(files: string[]): string[] {
  const pr = workflow('pr-check.yml')
  const source = pr.jobs.changes.steps.find((step: { id?: string }) => step.id === 'filter').with
    .filters
  const filters = parse(source.includes('\n') ? source : read(source)) as Record<string, unknown[]>
  return Object.entries(filters)
    .filter(([name]) =>
      [
        'frontend',
        'gui_go',
        'cli_go',
        'rust',
        'quick_panel',
        'e2e',
        'membership',
        'tooling',
      ].includes(name)
    )
    .filter(([, rules]) =>
      files.some(file => picomatch(rules.flat(Infinity) as string[], { dot: true })(file))
    )
    .map(([name]) => name)
    .sort()
}

describe('PR check routing', () => {
  it.each([
    ['coverage', 'rust'],
    ['rust', 'rust'],
    ['e2e', 'e2e'],
    ['membership', 'membership'],
    ['quick-panel-windows', 'quick_panel'],
    ['cli-go', 'cli_go'],
    ['gui-go', 'gui_go'],
    ['tooling', 'tooling'],
  ])('connects %s to its %s routing result', (job, filter) => {
    expect(workflow('pr-check.yml').jobs[job].if).toContain(`outputs.${filter} == 'true'`)
  })

  it.each([
    ['Go CLI', ['apps/cli-go/internal/commands/tree.go'], ['cli_go', 'e2e', 'membership']],
    ['frontend', ['apps/gui-go/frontend/src/App.tsx'], ['frontend']],
    ['Go host', ['apps/gui-go/main.go'], ['gui_go']],
    ['Quick Panel client', ['crates/uc-daemon-client/src/lib.rs'], ['quick_panel', 'rust']],
    [
      'shared Go module',
      ['packages/desktop-host-go/daemonproc/client.go'],
      ['cli_go', 'e2e', 'gui_go', 'membership'],
    ],
    ['documentation', ['apps/cli-go/README.md', 'docs-site/content/docs/zh/cli/reference.mdx'], []],
    ['release workflow', ['.github/workflows/release.yml'], ['tooling']],
    ['schema', ['schema/openapi.json'], ['frontend', 'rust']],
    ['CLI E2E script', ['scripts/e2e/build-cli.sh'], ['e2e', 'membership', 'tooling']],
  ])('routes %s changes to their consumers', (_name, files, expected) => {
    expect(route(files)).toEqual(expected)
  })

  it('checks Windows quick panel consumers when a shared Rust dependency changes', () => {
    expect(route(['crates/uc-observability/src/lib.rs'])).toEqual([
      'e2e',
      'membership',
      'quick_panel',
      'rust',
    ])
    expect(route(['apps/quick-panel/src/platform/win32/mod.rs'])).toEqual(['quick_panel', 'rust'])
  })

  it.each(['Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'rust-toolchain.toml'])(
    'expands %s changes to Rust and integration consumers',
    file => {
      expect(route([file])).toEqual(
        expect.arrayContaining(['rust', 'quick_panel', 'e2e', 'membership'])
      )
    }
  )

  it('runs repository tooling tests once, including cache policy tests', () => {
    expect(fs.existsSync(path.join(root, '.github/workflows/cache-policy-check.yml'))).toBe(false)
    expect(
      workflow('pr-check.yml').jobs.tooling.steps.some(
        (step: { run?: string }) => step.run === 'bun x vitest run'
      )
    ).toBe(true)
  })

  it('reuses frontend type checking while preserving the full local boundary check', () => {
    const scripts = JSON.parse(read('package.json')).scripts
    expect(scripts['check:host-boundary']).toBe('bun run typecheck && bun run check:host-structure')
    expect(scripts['check:host-structure']).not.toContain('tsc')
    expect(
      workflow('pr-check.yml').jobs['gui-go'].steps.some((step: { run?: string }) =>
        step.run?.includes('bun run check:host-structure')
      )
    ).toBe(true)
  })

  it('uses one PR frontend suite and hands its bundle to the host compile job', () => {
    const pr = workflow('pr-check.yml')
    const frontend = workflow('frontend-coverage.yml')
    expect(pr.jobs['frontend-tests']).toBeUndefined()
    expect(pr.jobs.frontend.uses).toBe('./.github/workflows/frontend-coverage.yml')
    expect(frontend.on.pull_request).toBeUndefined()
    expect(frontend.on.push.branches).toEqual(['main'])
    expect(frontend.on.workflow_call).toBeDefined()
    const runs = frontend.jobs.coverage.steps.map((step: { run?: string }) => step.run ?? '')
    expect(runs.filter((run: string) => run.includes('test:coverage:frontend'))).toHaveLength(1)
    expect(pr.jobs['gui-go'].needs).toContain('frontend')
    const hostSteps = pr.jobs['gui-go'].steps
    const upload = frontend.jobs.coverage.steps.find((step: { uses?: string }) =>
      step.uses?.startsWith('actions/upload-artifact@')
    )
    const download = hostSteps.find((step: { uses?: string }) =>
      step.uses?.startsWith('actions/download-artifact@')
    )
    expect(upload.with.name).toBe(download.with.name)
    expect(upload.with.name).not.toContain('run_attempt')
    expect(upload.with.overwrite).toBe(true)
    expect(hostSteps.some((step: { run?: string }) => step.run?.includes('frontend build'))).toBe(
      false
    )
    expect(
      hostSteps.some((step: { uses?: string }) =>
        step.uses?.startsWith('actions/download-artifact@')
      )
    ).toBe(true)
  })

  it.each(['coverage.yml', 'membership-e2e.yml'])(
    'routes %s PR runs through the shared change detector',
    file => {
      const target = workflow(file)
      expect(target.on.pull_request).toBeUndefined()
      expect(target.on.workflow_call).toBeDefined()
      expect(
        Object.values(workflow('pr-check.yml').jobs).some(
          (job: unknown) => (job as { uses?: string }).uses === `./.github/workflows/${file}`
        )
      ).toBe(true)
    }
  )
})
