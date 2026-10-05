import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { resolveAppEnv } from '../ci/resolve-app-env.mjs'

const workflows = path.resolve(__dirname, '../../.github/workflows')
const read = (file: string) => fs.readFileSync(path.join(workflows, file), 'utf8')

// Body of the step whose `- name:` line is exactly `name`.
function step(source: string, name: string) {
  const start = source.indexOf(`- name: ${name}\n`)
  expect(start, `step "${name}"`).toBeGreaterThan(-1)
  // A step ends at the next line indented no deeper than a step item
  // (the next step, or the next job / top-level key).
  const next = source.slice(start + 1).search(/\n {0,6}\S/)
  return source.slice(start, next === -1 ? undefined : start + 1 + next)
}

// Jobs of a workflow file, keyed by job id, with each job's raw text.
function jobs(source: string) {
  const body = source.slice(source.indexOf('\njobs:\n') + '\njobs:\n'.length)
  const parts = body.split(/^ {2}([a-z][\w-]*):\n/m)
  const result: Record<string, string> = {}
  for (let i = 1; i < parts.length; i += 2) result[parts[i]] = parts[i + 1]
  return result
}

function needsOf(job: string): string[] {
  const match = job.match(/^ {4}needs: (.*)$/m)
  return match
    ? match[1]
        .replace(/[[\]]/g, '')
        .split(',')
        .map(item => item.trim())
    : []
}

const TELEMETRY_ENV = [
  'SENTRY_DSN: ${{ secrets.SENTRY_DSN }}',
  'POSTHOG_PROJECT_KEY: ${{ secrets.POSTHOG_PROJECT_KEY }}',
  'APP_ENV: ${{ steps.resolve-env.outputs.app_env }}',
]

describe('APP_ENV resolution', () => {
  it.each([
    ['stable', 'production'],
    ['', 'production'],
    ['alpha', 'alpha'],
    ['beta', 'beta'],
    ['rc', 'rc'],
  ])('maps channel %j to %s', (channel, appEnv) => {
    expect(resolveAppEnv(channel)).toEqual({ appEnv, known: true })
  })

  it('falls back to production for unknown channels and reports it', () => {
    expect(resolveAppEnv('nightly')).toEqual({ appEnv: 'production', known: false })
  })
})

describe('shipped uniclipd builds', () => {
  it('embeds the same telemetry env in the app sidecar and the CLI daemon', () => {
    const sidecar = step(read('build.yml'), 'prepare uniclipd sidecar')
    const cli = step(read('build-cli.yml'), 'build uniclipd')
    expect(cli).toContain('cargo build --release -p uc-daemon --bin uniclipd')
    for (const line of TELEMETRY_ENV) {
      expect(sidecar).toContain(line)
      expect(cli).toContain(line)
    }
    for (const source of [read('build.yml'), read('build-cli.yml')]) {
      expect(step(source, 'resolve APP_ENV from channel')).toContain(
        'run: node scripts/ci/resolve-app-env.mjs'
      )
    }
  })

  it('packages macOS and Windows x64 CLI archives from the release app sidecar', () => {
    const build = read('build.yml')
    const cli = jobs(build)['build-cli']
    expect(cli).toContain(
      "if: inputs.package_cli && inputs.build_mode == 'release' && needs.setup-matrix.outputs.cli-matrix != '[]'"
    )
    // Only the targets that share a triple with the app reuse its sidecar.
    expect(step(build, 'Select CLI targets')).toContain(
      'e.platform === "macos-latest" || e.target === "x86_64-pc-windows-msvc"'
    )
    expect(step(cli, 'package CLI binary')).toContain(
      '"apps/gui/src-tauri/binaries/uniclipd-${{ matrix.target }}$EXE"'
    )
    // The CLI must reuse the sidecar, never rebuild the daemon in the app job.
    expect(build.match(/-p uc-daemon/g) ?? []).toHaveLength(0)

    const release = read('release.yml')
    expect(release).toContain('package_cli: true')
    expect(release).toContain('linux_only: true')
    expect(step(read('build-cli.yml'), 'Select CLI targets')).toContain(
      'e.target.includes("-linux-musl")'
    )
  })
})

describe('build job graph', () => {
  it.each([
    ['build.yml', 'build-gui'],
    ['build.yml', 'build-cli'],
    ['build-cli.yml', 'build-cli'],
  ])('%s builds the sidecar first, then runs %s after it', (file, job) => {
    const all = jobs(read(file))
    expect(needsOf(all['build-sidecar'])).toEqual(['setup-matrix'])
    expect(needsOf(all[job])).toContain('build-sidecar')
  })

  it('runs the GUI build and the Go CLI build in parallel', () => {
    const all = jobs(read('build.yml'))
    expect(needsOf(all['build-gui'])).not.toContain('build-cli')
    expect(needsOf(all['build-cli'])).not.toContain('build-gui')
  })

  it('builds the daemon only in the sidecar job', () => {
    const build = jobs(read('build.yml'))
    expect(build['build-sidecar']).toContain('run: node scripts/prepare-sidecars.mjs')
    for (const job of ['build-gui', 'build-cli']) {
      expect(build[job]).not.toContain('run: node scripts/prepare-sidecars.mjs')
      expect(build[job]).not.toContain('cargo build')
    }
    const cliWorkflow = jobs(read('build-cli.yml'))
    expect(cliWorkflow['build-sidecar']).toContain('--bin uniclipd')
    expect(cliWorkflow['build-cli']).not.toContain('cargo ')
  })

  it('builds the user-facing CLI from the Go module, without a Rust toolchain', () => {
    for (const file of ['build.yml', 'build-cli.yml']) {
      const cli = jobs(read(file))['build-cli']
      expect(cli).toContain('uses: actions/setup-go@v6')
      expect(cli).toContain('go-version-file: apps/cli-go/go.mod')
      expect(step(cli, 'build Go CLI')).toContain('scripts/ci/build-go-cli.sh')
      expect(cli).not.toContain('install Rust toolchain')
    }
  })

  it('hands the sidecar over as one tar, never as loose release-like files', () => {
    // release.yml collects every *.exe / *.json / *.sig under the downloaded
    // artifacts, so a loose uniclipd-*.exe would be published as a release asset.
    for (const [file, name] of [
      ['build.yml', 'sidecar-${{ matrix.target }}'],
      ['build-cli.yml', 'cli-sidecar-${{ matrix.target }}'],
    ]) {
      const all = jobs(read(file))
      const upload = step(all['build-sidecar'], 'upload sidecar artifact')
      expect(upload).toContain('path: sidecar.tar')
      expect(upload).toContain(`name: ${name}`)
      expect(all['build-sidecar']).not.toMatch(/path: .*binaries/)
      for (const consumer of Object.keys(all).filter(job =>
        needsOf(all[job]).includes('build-sidecar')
      )) {
        expect(step(all[consumer], 'download uniclipd sidecar')).toContain(`name: ${name}`)
      }
    }
  })
})

describe('Rust development CLI stays out of production builds', () => {
  const root = path.resolve(__dirname, '../..')
  const production = [
    '.github/workflows/build.yml',
    '.github/workflows/build-cli.yml',
    '.github/workflows/release.yml',
    '.github/workflows/alpha-build.yml',
    '.github/workflows/build-server-image.yml',
    'deploy/vps/Dockerfile',
    'scripts/prepare-sidecars.mjs',
    'scripts/build-npm-packages.mjs',
    'scripts/ci/package-cli.sh',
    'scripts/ci/build-go-cli.sh',
  ]

  it.each(production)('%s does not build or package the Rust CLI crate', file => {
    const source = fs.readFileSync(path.join(root, file), 'utf8')
    expect(source).not.toMatch(/uc-dev-cli|\buc-cli\b/)
  })

  it('is a development crate: outside default-members and never published', () => {
    const root = path.resolve(__dirname, '../..')
    const manifest = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8')
    const list = (key: string) =>
      [
        ...(
          manifest.match(new RegExp(`^${key} = \\[\\n([\\s\\S]*?)\\n\\]`, 'm'))?.[1] ?? ''
        ).matchAll(/"([^"]+)"/g),
      ].map(match => match[1])
    const tools = ['tools/uc-dev-cli', 'crates/uc-cli-macros']
    const members = list('members')
    for (const tool of tools) expect(members).toContain(tool)
    // `--workspace` and `-p` still reach the tools, a bare `cargo build` does not,
    // and nothing else may silently drop out of the default set.
    expect(list('default-members')).toEqual(members.filter(member => !tools.includes(member)))
    for (const file of ['tools/uc-dev-cli/Cargo.toml', 'crates/uc-cli-macros/Cargo.toml']) {
      expect(fs.readFileSync(path.join(root, file), 'utf8'), file).toMatch(/^publish = false$/m)
    }
  })

  it('no Cargo manifest depends on it', () => {
    const manifests: string[] = []
    const walk = (dir: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        if (entry.name === 'target' || entry.name === 'node_modules') continue
        const full = path.join(dir, entry.name)
        if (entry.isDirectory()) walk(full)
        else if (entry.name === 'Cargo.toml') manifests.push(full)
      }
    }
    for (const dir of ['apps', 'crates', 'tools']) walk(path.join(root, dir))
    const own = path.join(root, 'tools/uc-dev-cli/Cargo.toml')
    for (const manifest of manifests.filter(file => file !== own)) {
      // A dependency declaration (`uc-dev-cli = ...` or `package = "uc-dev-cli"`), not a mention.
      expect(fs.readFileSync(manifest, 'utf8'), manifest).not.toMatch(
        /^\s*uc-dev-cli\s*=|package\s*=\s*"uc-dev-cli"/m
      )
    }
  })
})

describe('E2E harness drives the Go CLI', () => {
  const root = path.resolve(__dirname, '../..')
  const text = (file: string) => fs.readFileSync(path.join(root, file), 'utf8')
  const scripts = fs
    .readdirSync(path.join(root, 'scripts/e2e'))
    .filter(file => file.endsWith('.sh'))
    .map(file => `scripts/e2e/${file}`)

  it.each(scripts)('%s never names the retired Rust CLI crate or binary', file => {
    expect(text(file)).not.toMatch(/uc-cli\b|uniclipboard-cli|--bin uniclip\b/)
  })

  it.each(scripts)('%s runs development commands through DEV_CLI, not CLI', file => {
    // `dev seed-clipboard`, `dev dump-clipboard` and `mobile debug` exist only in uc-dev-cli.
    for (const line of text(file).split('\n')) {
      if (/\b(dev (seed|dump)-clipboard|mobile debug)\b/.test(line) && line.includes('"$CLI"')) {
        throw new Error(`${file}: development command through the user-facing CLI: ${line.trim()}`)
      }
    }
  })

  it('builds the Go CLI into the directory the suites resolve it from', () => {
    const build = text('scripts/e2e/build-cli.sh')
    expect(build).toContain(
      'go build -buildvcs=false -o "$TARGET_DIR/debug/uniclip$EXE" ./cmd/uniclip'
    )
    const binaries = text('tests/e2e/src/binaries.rs')
    expect(binaries).toContain('exe_name("uniclip")')
    expect(binaries).toContain('exe_name("uc-dev-cli")')
    expect(binaries).toContain('UC_E2E_CLI')
    expect(binaries).toContain('UC_E2E_DEV_CLI')
  })

  it.each([
    ['pr-check.yml', 'Run E2E tests'],
    ['membership-e2e.yml', 'Run membership matrix'],
  ])('%s builds the Go CLI before the "%s" step', (file, runStep) => {
    const workflow = text(`.github/workflows/${file}`)
    const build = workflow.indexOf('run: scripts/e2e/build-cli.sh')
    expect(build).toBeGreaterThan(workflow.indexOf('uses: actions/setup-go@v6'))
    expect(workflow.indexOf('uses: actions/setup-go@v6')).toBeGreaterThan(-1)
    expect(build).toBeGreaterThan(-1)
    expect(build).toBeLessThan(workflow.indexOf(`- name: ${runStep}\n`))
    // The daemon build must not drag the development CLI along.
    expect(workflow).not.toMatch(/cargo build -p uc-daemon -p uc-dev-cli/)
  })

  it('points the dev-CLI override at uc-dev-cli', () => {
    expect(text('.github/workflows/pr-check.yml')).toContain(
      'UC_E2E_DEV_CLI: ${{ github.workspace }}/target/e2e-dev/debug/uc-dev-cli'
    )
  })
})

describe('Go CLI build info', () => {
  it('matches the workspace version and the daemon API revision', () => {
    const root = path.resolve(__dirname, '../..')
    const read = (file: string) => fs.readFileSync(path.join(root, file), 'utf8')
    const version = read('Cargo.toml').match(/\[workspace\.package\]\s*version = "([^"]+)"/)?.[1]
    const revision = read('crates/uc-daemon-contract/src/lib.rs').match(
      /DAEMON_API_REVISION: &str =\s*"([^"]+)"/
    )?.[1]
    const info = read('apps/cli-go/internal/buildinfo/buildinfo.go')
    expect(version).toBeTruthy()
    expect(revision).toBeTruthy()
    expect(info).toContain(`const PackageVersion = "${version}"`)
    expect(info).toContain(`const DaemonAPIRevision = "${revision}"`)
  })
})

describe('CLI daemon debug symbols', () => {
  const source = read('build-cli.yml')

  it('uploads daemon debug symbols after building and before packaging', () => {
    const upload = source.indexOf('- name: Upload Sentry debug symbols')
    expect(upload).toBeGreaterThan(source.indexOf('--bin uniclipd'))
    expect(upload).toBeLessThan(source.indexOf('- name: package CLI binary'))
    expect(step(source, 'Upload Sentry debug symbols')).toContain(
      'sentry-cli debug-files upload --include-sources "$TARGET_DIR"'
    )
  })
})

// Run a setup-matrix step's `run:` block with the given expressions substituted
// and return what it wrote to GITHUB_OUTPUT.
function runStep(body: string, expressions: Record<string, string>, env: Record<string, string>) {
  const lines = body.slice(body.indexOf('run: |\n') + 'run: |\n'.length).split('\n')
  const indent = lines[0].match(/^ */)?.[0].length ?? 0
  let script = lines.map(line => line.slice(indent)).join('\n')
  for (const [expression, value] of Object.entries(expressions)) {
    script = script.split(expression).join(value)
  }
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-cli-matrix-'))
  try {
    const output = path.join(directory, 'output')
    const result = spawnSync('bash', ['-e', '-c', script], {
      encoding: 'utf8',
      env: { ...process.env, ...env, GITHUB_OUTPUT: output },
    })
    expect(result.status, result.stdout + result.stderr).toBe(0)
    const last = fs.readFileSync(output, 'utf8').trim().split('\n').pop() ?? ''
    return JSON.parse(last.replace(/^matrix=/, '')) as Array<{ target: string }>
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
}

describe('release CLI matrix', () => {
  const source = read('build-cli.yml')
  const releasePlatforms = [
    ...read('release.yml').matchAll(/^ {10}- '((?:all|macos|ubuntu|windows)[^']*)'$/gm),
  ].map(match => match[1])

  function cliTargets(platform: string, linuxOnly: boolean) {
    const matrix = runStep(
      step(source, 'Generate matrix'),
      { "${{ inputs.platform || 'all' }}": platform },
      { WINDOWS_RUNNER: 'windows-latest' }
    )
    return runStep(
      step(source, 'Select CLI targets'),
      {},
      { MATRIX: JSON.stringify(matrix), LINUX_ONLY: String(linuxOnly) }
    ).map(entry => entry.target)
  }

  it('accepts every platform the Release workflow can pass', () => {
    expect(releasePlatforms).toContain('windows-arm64')
    for (const platform of releasePlatforms) {
      for (const target of cliTargets(platform, true)) {
        expect(target).toMatch(/-linux-musl$/)
      }
    }
  })

  it('maps the app-only Windows platform names to CLI targets', () => {
    expect(cliTargets('windows-x86_64', false)).toEqual(['x86_64-pc-windows-msvc'])
    expect(cliTargets('windows-arm64', false)).toEqual([])
    expect(cliTargets('all', true)).toEqual([
      'x86_64-unknown-linux-musl',
      'aarch64-unknown-linux-musl',
    ])
  })
})
