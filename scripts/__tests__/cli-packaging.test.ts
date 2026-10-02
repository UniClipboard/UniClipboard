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
    const cli = step(read('build-cli.yml'), 'build CLI binary')
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
    const guard =
      "if: inputs.package_cli && inputs.build_mode == 'release' && (matrix.platform == 'macos-latest' || matrix.target == 'x86_64-pc-windows-msvc')"
    for (const name of ['build CLI binary', 'package CLI binary', 'upload CLI artifact']) {
      expect(step(build, name)).toContain(guard)
    }
    expect(step(build, 'package CLI binary')).toContain(
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
