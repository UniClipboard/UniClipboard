import fs from 'node:fs'
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
      '"src-tauri/binaries/uniclipd-${{ matrix.target }}$EXE"'
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
