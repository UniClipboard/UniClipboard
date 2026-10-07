#!/usr/bin/env node

// Guards the retirement of the Tauri desktop host: the Go/Wails host in apps/gui-go
// is the only desktop shell, so no Tauri host crate, config, build tooling or path
// reference may come back. `@tauri-apps/*` npm package ids stay on purpose: the shared
// React frontend imports them by name and apps/gui-go/vite.config.ts aliases each one
// to a Wails-backed adapter, so they are an import boundary, not a dependency on Tauri.

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')

const RETIRED_PATHS = [
  'apps/gui/src-tauri',
  'apps/gui/e2e/run.mjs',
  'apps/gui/e2e/specs',
  'crates/uc-tauri',
  'third_party/tao',
  'patches/tao-0.35.3-pr1207.diff',
]

// Cargo packages that only exist to host or support the retired shell.
const RETIRED_CARGO = /^(tauri($|-)|tao$|wry$|uc-tauri$|uniclipboard$)/

// Retired build tooling and scripts: no tracked file may mention them.
const RETIRED_TOOLING = [
  'tauri.e2e.conf',
  'third_party/tao',
  'tauri-build',
  'tauri-cli',
  'prepare-sidecars',
  'prepare-linux-bundle',
  'tauri:dev',
  'tauri:build',
  'bun tauri',
  'tauri-action',
  'alpha-build',
]

// Names of the retired host crates, directory and config. Source files (Go, Rust, TypeScript) may
// keep them in comments that cite where a behaviour or a protocol came from; every other tracked
// file (manifests, workflows, scripts, packaging, live documentation) must not.
const RETIRED_NAMES = ['src-tauri', 'uc-tauri', 'uc_tauri', 'tauri.conf.json']
const SOURCE_COMMENT_GLOBS = ['*.go', '*.rs', '*.ts', '*.tsx']

// npm packages that only served the retired host (the `@tauri-apps/api` and plugin packages the
// shared frontend imports by name stay: apps/gui-go/vite.config.ts aliases each to a Wails adapter).
const RETIRED_NPM = [/^@tauri-apps\/cli$/, /^@wdio\/tauri-/]

// Packaging channels that still build the retired Tauri host from source. Their port to the Go
// host needs the packaging work tracked in docs/architecture/gui-go-tauri-retirement.md and
// cannot be verified without those channels, so they are listed here until that work lands.
// aur.yml publishes packaging/aur/** to the AUR on every push to main, so it is not edited blind.
const PENDING_PORT = [
  'snap/snapcraft.yaml',
  'packaging/aur/uniclipboard-git/PKGBUILD',
  'docs/packaging/AUR.md',
]

// Tracked paths that may keep a historical mention: research notes, retired plans, release
// history, decision records and dated notes under docs/, the retirement record, and this guard.
const HISTORY_EXCLUDES = [
  '.planning',
  'plans',
  'docs-site',
  'docs/changelog',
  'docs/fixes',
  'docs/p2p',
  'docs/specs',
  'docs/planning',
  'docs/uat',
  'docs/development',
  'docs/security-audit.md',
  ':(glob)docs/architecture/adr-*.md',
  'docs/architecture/gui-go-tauri-retirement.md',
  // Whole-file historical records of the retired host; each carries a banner saying so.
  'docs/guides/github-releases-updater.md',
  'docs/guides/device-group-gui-testing.md',
  'docs/architecture/commands-layer-specification.md',
  'docs/architecture/commands-status.md',
  'docs/architecture/bootstrap.md',
  'docs/architecture/desktop-theme-preferences.md',
  'scripts/architecture/check-tauri-retired.mjs',
]

function git(args) {
  return execFileSync('git', args, { cwd: ROOT, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
}

function checkPaths(problems) {
  for (const path of RETIRED_PATHS) {
    if (existsSync(join(ROOT, path))) problems.push(`retired path still exists: ${path}`)
  }
}

function checkCargo(problems) {
  const metadata = JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version', '1', '--locked'], {
      cwd: ROOT,
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
    })
  )
  const retired = new Set(
    metadata.packages.filter(pkg => RETIRED_CARGO.test(pkg.name)).map(pkg => pkg.name)
  )
  for (const name of [...retired].sort()) problems.push(`retired cargo package resolved: ${name}`)
}

function scan(reference, excludes, problems) {
  const scope = ['.', ...excludes.map(path => `:(exclude,glob)${path}`)]
  let output = ''
  try {
    output = git(['grep', '-l', '-I', '-F', '-e', reference, '--', ...scope])
  } catch (error) {
    // `git grep` exits 1 when nothing matches; that is the passing case.
    if (error.status !== 1) throw error
  }
  for (const file of output.split('\n').filter(Boolean)) {
    problems.push(`${file}: references retired "${reference}"`)
  }
}

function checkNpm(problems) {
  for (const manifest of ['package.json', 'apps/gui/package.json', 'apps/gui-go/package.json']) {
    const content = JSON.parse(readFileSync(join(ROOT, manifest), 'utf8'))
    const dependencies = Object.keys({
      ...content.dependencies,
      ...content.devDependencies,
      ...content.optionalDependencies,
    })
    for (const name of dependencies) {
      if (RETIRED_NPM.some(pattern => pattern.test(name))) {
        problems.push(`${manifest}: depends on retired npm package ${name}`)
      }
    }
  }
}

function checkReferences(problems) {
  const history = HISTORY_EXCLUDES.map(path =>
    path.startsWith(':(glob)') ? path.slice(7) : `${path}/**`
  )
  const files = HISTORY_EXCLUDES.filter(path => !path.startsWith(':(glob)') && path.includes('.'))
  const excludes = [...history, ...files, ...PENDING_PORT]
  for (const reference of RETIRED_TOOLING) scan(reference, excludes, problems)
  const sourceExcludes = [...excludes, ...SOURCE_COMMENT_GLOBS.map(glob => `**/${glob}`)]
  for (const reference of RETIRED_NAMES) scan(reference, sourceExcludes, problems)
}

const problems = []
checkPaths(problems)
checkNpm(problems)
try {
  checkCargo(problems)
} catch (error) {
  problems.push(`cargo metadata failed: ${String(error.message ?? error).split('\n')[0]}`)
}
try {
  checkReferences(problems)
} catch (error) {
  problems.push(`reference scan failed: ${String(error.message ?? error)}`)
}

if (problems.length > 0) {
  console.error(`Tauri retirement check failed (${problems.length} problems):`)
  for (const problem of problems) console.error(`  - ${problem}`)
  process.exit(1)
}
console.log('Tauri retirement check passed')
