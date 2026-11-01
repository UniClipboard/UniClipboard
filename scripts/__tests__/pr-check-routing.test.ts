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
    .filter(([name]) => ['frontend', 'gui_go', 'cli_go', 'rust', 'tooling'].includes(name))
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
    ['cli-go', 'cli_go'],
    ['gui-go', 'gui_go'],
    ['tooling', 'tooling'],
  ])('connects %s to its %s routing result', (job, filter) => {
    expect(workflow('pr-check.yml').jobs[job].if).toContain(`outputs.${filter} == 'true'`)
  })

  it.each([
    ['Go CLI', ['apps/cli-go/internal/commands/tree.go'], ['cli_go']],
    ['frontend', ['apps/gui-go/frontend/src/App.tsx'], ['frontend']],
    ['Go host', ['apps/gui-go/main.go'], ['gui_go']],
    ['daemon client', ['crates/uc-daemon-client/src/lib.rs'], ['rust']],
    ['shared Go module', ['packages/desktop-host-go/daemonproc/client.go'], ['cli_go', 'gui_go']],
    ['documentation', ['apps/cli-go/README.md', 'docs-site/content/docs/zh/cli/reference.mdx'], []],
    ['release workflow', ['.github/workflows/release.yml'], ['tooling']],
    ['schema', ['schema/openapi.json'], ['frontend', 'rust']],
    ['CLI E2E script', ['scripts/e2e/build-cli.sh'], ['tooling']],
  ])('routes %s changes to their consumers', (_name, files, expected) => {
    expect(route(files)).toEqual(expected)
  })

  it('routes quick panel changes to the Rust check', () => {
    expect(route(['crates/uc-observability/src/lib.rs'])).toEqual(['rust'])
    expect(route(['apps/quick-panel/src/platform/win32/mod.rs'])).toEqual(['rust'])
  })

  it.each(['Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'rust-toolchain.toml'])(
    'expands %s changes to Rust checks',
    file => {
      expect(route([file])).toContain('rust')
    }
  )

  it('runs repository tooling tests once, including cache policy tests', () => {
    expect(fs.existsSync(path.join(root, '.github/workflows/cache-policy-check.yml'))).toBe(false)
    expect(
      workflow('pr-check.yml').jobs.tooling.steps.some(
        (step: { run?: string }) => step.run === 'bun x vitest run'
      )
    ).toBe(true)
    expect(
      workflow('pr-check.yml').jobs.tooling.steps.some((step: { run?: string }) =>
        step.run?.includes('UC_MBX_TEST_BINARY')
      )
    ).toBe(false)
    const native = workflow('mbx-snapshot-validation.yml').jobs['native-roundtrip']
    expect(native.strategy.matrix.runner).toContain('ubuntu-latest')
    expect(
      native.steps.some((step: { run?: string }) => step.run?.includes('UC_MBX_TEST_BINARY'))
    ).toBe(true)
  })

  it.each([
    ['tests/e2e/tests/clipboard_sync.rs', false],
    ['tests/e2e/tests/membership_convergence.rs', false],
    ['tests/e2e/README.md', false],
    ['tests/e2e/tests/legacy_file_key_store.rs', true],
    ['tests/e2e/src/profile.rs', true],
    ['tests/e2e/Cargo.toml', true],
    [
      'tests/e2e/fixtures/upgrades/v0.19.3/macos-aarch64/single-node-mixed-content/manifest.json',
      true,
    ],
    ['scripts/e2e/build-cli.sh', true],
    ['apps/cli-go/internal/commands/get.go', true],
    ['packages/desktop-host-go/daemonproc/conn.go', true],
    ['Cargo.lock', true],
  ])('routes legacy key store input %s with relevance %s', (file, relevant) => {
    const rules = workflow('legacy-file-key-store-e2e.yml').on.pull_request.paths
    expect(picomatch(rules, { dot: true })(file)).toBe(relevant)
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

  it('routes the shared coverage workload to Rust checks and native coverage validation', () => {
    expect(route(['.github/actions/rust-coverage/action.yml'])).toContain('rust')
    const native = workflow('mbx-snapshot-validation.yml')
    expect(native.on.pull_request.paths).toContain('.github/actions/rust-coverage/**')
    expect(
      native.jobs['coverage-roundtrip'].steps.some((step: { run?: string }) =>
        step.run?.includes('test_coverage_snapshot_executes_tests_and_regenerates_lcov')
      )
    ).toBe(true)
  })

  it('routes coverage through the PR change detector', () => {
    const target = workflow('coverage.yml')
    expect(target.on.pull_request).toBeUndefined()
    expect(target.on.workflow_call).toBeDefined()
    expect(workflow('pr-check.yml').jobs.coverage.uses).toBe('./.github/workflows/coverage.yml')
  })

  it('runs integration and Linux packaging nightly without a PR trigger', () => {
    const pr = workflow('pr-check.yml')
    const nightly = workflow('nightly-integration.yml')
    expect(nightly.on.schedule).toEqual([{ cron: '23 18 * * *' }])
    expect(nightly.on.workflow_dispatch).toBeDefined()
    expect(pr.on.workflow_call).toBeDefined()
    expect(nightly.jobs.integration.uses).toBe('./.github/workflows/pr-check.yml')
    expect(nightly.jobs['linux-gui'].uses).toBe('./.github/workflows/package-linux-gui.yml')
    expect(nightly.jobs['linux-gui'].with.run_acceptance).toBe(true)
    expect(pr.jobs.changes.if).toBe("github.event_name == 'pull_request'")
    for (const job of ['membership', 'quick-panel-windows', 'e2e']) {
      expect(pr.jobs[job].if).toBe("github.event_name != 'pull_request'")
      expect(pr.jobs[job].needs).toBeUndefined()
    }
    expect(pr.jobs.membership.with.tier).toBe('nightly')
    expect(pr.jobs.membership.secrets).toBe('inherit')
    expect(workflow('membership-e2e.yml').on.schedule).toBeUndefined()
    expect(workflow('package-linux-gui.yml').on.pull_request).toBeUndefined()
  })
})

describe('TesterArmy PR routing', () => {
  const jobs = {
    testerarmy_browser: ['testerarmy-browser', 'tester-army-e2e.yml'],
    testerarmy_backend: ['testerarmy-backend', 'tester-army-e2e.yml'],
    testerarmy_native_macos: ['testerarmy-native-macos', 'tester-army-native.yml'],
    testerarmy_native_linux: ['testerarmy-native-linux', 'tester-army-native.yml'],
    testerarmy_native_windows: ['testerarmy-native-windows', 'tester-army-native.yml'],
    testerarmy_windows_product: ['testerarmy-windows-product', 'tester-army-native.yml'],
  }
  const routed = (file: string) => {
    const filters = parse(read('.github/ci-paths.yml'))
    return Object.keys(jobs).filter(name =>
      picomatch((filters[name] ?? []).flat(Infinity), { dot: true })(file)
    )
  }

  it.each([
    ['docs/testing/tester-army-migration.md', []],
    ['apps/gui-go/e2e/tester-army-native/wkwebview.swift', ['testerarmy_native_macos']],
    ['apps/gui-go/e2e/tester-army-native/webkitgtk.py', ['testerarmy_native_linux']],
    ['apps/gui-go/e2e/tester-army-native/webview2.mjs', ['testerarmy_native_windows']],
    ['apps/gui-go/e2e/tester-army-native/windows-product.e2e.mjs', ['testerarmy_windows_product']],
    ['apps/gui-go/cmd/webview-probe/main.go', ['testerarmy_native_windows']],
    ['apps/gui-go/main.go', ['testerarmy_windows_product']],
    [
      'apps/gui-go/frontend/src/App.tsx',
      ['testerarmy_browser', 'testerarmy_backend', 'testerarmy_windows_product'],
    ],
    ['apps/daemon/src/main.rs', ['testerarmy_backend', 'testerarmy_windows_product']],
    ['vendor/engine', ['testerarmy_backend', 'testerarmy_windows_product']],
    ['.cargo/config.toml', ['testerarmy_backend', 'testerarmy_windows_product']],
    ['apps/cli-go/internal/commands/get.go', ['testerarmy_backend', 'testerarmy_windows_product']],
    ['e2e.macos-history.config.ts', ['testerarmy_native_macos']],
  ])('routes %s only to its consumers', (file, expected) => {
    expect(routed(file)).toEqual(expected)
  })

  it.each(Object.entries(jobs))(
    'connects %s through the shared detector',
    (filter, [job, file]) => {
      const pr = workflow('pr-check.yml')
      expect(pr.jobs.changes.outputs[filter]).toContain(`steps.filter.outputs.${filter}`)
      expect(pr.jobs[job].needs).toBe('changes')
      expect(pr.jobs[job].if).toContain(`outputs.${filter} == 'true'`)
      expect(pr.jobs[job].uses).toBe(`./.github/workflows/${file}`)
      const target = workflow(file)
      expect(target.on.pull_request).toBeUndefined()
      expect(target.on.workflow_call).toBeDefined()
      expect(target.on).toHaveProperty('workflow_dispatch')
      expect(target.concurrency['cancel-in-progress']).toBe(true)
    }
  )

  it('separates browser, backend, platform probes and product execution', () => {
    const pr = workflow('pr-check.yml')
    expect(pr.jobs['testerarmy-browser'].with).toMatchObject({
      run_browser: true,
      run_backend: false,
    })
    expect(pr.jobs['testerarmy-backend'].with).toMatchObject({
      run_browser: false,
      run_backend: true,
    })
    for (const [platform, runner] of [
      ['macos', 'macos-latest'],
      ['linux', 'ubuntu-latest'],
      ['windows', 'windows-latest'],
    ]) {
      expect(pr.jobs[`testerarmy-native-${platform}`].with).toMatchObject({
        run_capability: true,
        capability_runner: runner,
        run_windows_product: false,
      })
    }
    expect(pr.jobs['testerarmy-windows-product'].with).toMatchObject({
      run_capability: false,
      run_windows_product: true,
    })
    for (const job of Object.values(workflow('tester-army-e2e.yml').jobs) as { if: string }[]) {
      expect(job.if).toContain("github.event_name == 'workflow_dispatch'")
      expect(job.if).toContain('inputs.run_')
    }
    const native = workflow('tester-army-native.yml')
    expect(native.jobs.capability.if).toContain('inputs.run_capability')
    expect(native.jobs['windows-product'].if).toContain('inputs.run_windows_product')
  })
})
