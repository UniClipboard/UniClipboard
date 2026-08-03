import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

const projectRoot = path.resolve(__dirname, '../..')

function read(relativePath: string): string {
  return fs.readFileSync(path.join(projectRoot, relativePath), 'utf8')
}

describe('membership E2E hardening', () => {
  it('passes the workflow tier through a quoted environment variable', () => {
    const workflow = read('.github/workflows/membership-e2e.yml')

    expect(workflow).toContain("MEMBERSHIP_TIER: ${{ inputs.tier || 'nightly' }}")
    expect(workflow).toContain('run: bash scripts/e2e/run-membership-matrix.sh "$MEMBERSHIP_TIER"')
    expect(workflow).not.toContain(
      'run: bash scripts/e2e/run-membership-matrix.sh "${{ inputs.tier'
    )
  })

  it('preserves aggregate failures and explains a missing legacy release', () => {
    const script = read('scripts/e2e/run-membership-matrix.sh')

    expect(script).not.toMatch(
      /^\s*set\b.*(?:\s-[A-Za-z]*e[A-Za-z]*(?:\s|$)|\s-o\s+errexit(?:\s|$))/m
    )
    expect(script).toContain('UC_E2E_LEGACY_RELEASE_DIR is not set; H1-H4 cannot run')
    expect(script).toContain('H1-H4 were required but could not run')
  })

  it('guards the E2E-only rendezvous override and release preparation', () => {
    const wiring = read('crates/uc-bootstrap/src/wiring/desktop_host.rs')
    const releases = read('tests/e2e/src/releases.rs')
    const convergence = read('tests/e2e/tests/membership_convergence.rs')

    expect(wiring).toContain('engine_config.with_rendezvous_base_url(base_url.trim().to_string())')
    expect(releases).toMatch(/\.timeout\((?:std::time::)?Duration::from_secs\(60\)\)/)
    expect(convergence.match(/terminate_child\(&mut child\)/g)).toHaveLength(2)
  })

  it('uses the production log resolver for profile cleanup', () => {
    const profile = read('tests/e2e/src/profile.rs')
    const manifest = read('tests/e2e/Cargo.toml')

    expect(profile).toContain('uc_app_paths::app_log_dir_for_profile(Some(profile))')
    expect(manifest).toContain('uc-app-paths = { path = "../../crates/uc-app-paths" }')
  })

  it('runs the cargo audit exception guard in CI', () => {
    const packageJson = JSON.parse(read('package.json')) as { scripts: Record<string, string> }
    const workflow = read('.github/workflows/pr-check.yml')
    const guard = read('scripts/architecture/check-cargo-audit-exceptions.mjs')
    const guardPath = path.join(
      projectRoot,
      'scripts/architecture/check-cargo-audit-exceptions.mjs'
    )

    expect(fs.existsSync(guardPath)).toBe(true)
    expect(packageJson.scripts['check:cargo-audit-exceptions']).toBe(
      'node scripts/architecture/check-cargo-audit-exceptions.mjs'
    )
    expect(workflow).toContain('run: bun run check:cargo-audit-exceptions')
    expect(guard).not.toContain("run('git'")
    expect(guard).toContain("const RUST_SOURCE_ROOTS = ['apps', 'crates', 'src-tauri']")
    expect(guard).toContain("'x86_64-unknown-linux-gnu'")
    expect(guard).toContain("'x86_64-pc-windows-msvc'")
    expect(guard).toContain("'x86_64-apple-darwin'")
    expect(guard).toContain("'wasm32-unknown-unknown'")
  })
})
