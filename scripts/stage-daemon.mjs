#!/usr/bin/env node
// Build the `uniclipd` daemon for a target triple and stage it, under the name
// `uniclipd-<target-triple>`, in `target/sidecar-staging/`. On macOS and Windows it also stages the native
// quick panel helper `uniclip-quick-panel` the same way.
//
// ADR-008 D13 ships `uniclipd` inside the GUI installer so the GUI (and the CLI) can spawn it as
// a *sibling* of the app executable — see `uc-daemon-local` `spawn.rs::resolve_daemon_exe_path`,
// whose first strategy is "look for `uniclipd` next to the current exe". The packagers
// (apps/gui-go/packaging/*, apps/gui-go/e2e/package_*.py) and the CLI archive job consume the
// staged files and place them next to the executable. The local macOS build.sh also invokes
// this owner with --debug and consumes that staging result.
//
// It is invoked by CI (build.yml), passing the same `--target <triple>` the
// packaging job uses (matrix.args), and locally for a native build.
//
// Usage:
//   node scripts/stage-daemon.mjs [--target <triple>] [--component daemon|helper|all] [--debug] [--timings]

import { execFileSync } from 'node:child_process'
import { chmodSync, copyFileSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const stagingDir = join(repoRoot, 'target', 'sidecar-staging')

function parseArgs(argv) {
  let target = ''
  let release = true
  let timings = false
  let component = 'all'
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if (arg === '--target') {
      target = argv[++i] ?? ''
    } else if (arg.startsWith('--target=')) {
      target = arg.slice('--target='.length)
    } else if (arg === '--component') {
      component = argv[++i] ?? ''
    } else if (arg === '--debug') {
      release = false
    } else if (arg === '--release') {
      release = true
    } else if (arg === '--timings') {
      timings = true
    }
    // Unknown args are ignored on purpose so callers can forward the packaging
    // job's `${{ matrix.args }}` verbatim (it is either `--target <triple>` or empty).
  }
  if (!['all', 'daemon', 'helper'].includes(component)) {
    throw new Error(`Unknown sidecar component: ${component}`)
  }
  return { target, release, timings, component }
}

function hostTriple() {
  // rustc 1.84+ prints the canonical host tuple directly; older toolchains
  // need the verbose-version `host:` line parsed out. Run inside the repo
  // root so the pinned toolchain's rustc answers.
  try {
    return execFileSync('rustc', ['--print', 'host-tuple'], {
      cwd: repoRoot,
      encoding: 'utf8',
    }).trim()
  } catch {
    const verbose = execFileSync('rustc', ['-Vv'], {
      cwd: repoRoot,
      encoding: 'utf8',
    })
    const match = verbose.match(/^host:\s*(.+)$/m)
    if (!match) {
      throw new Error('could not determine host target triple from `rustc -Vv`')
    }
    return match[1].trim()
  }
}

const { target, release, timings, component } = parseArgs(process.argv.slice(2))
const triple = target || hostTriple()
const isWindows = triple.includes('windows')
const exeSuffix = isWindows ? '.exe' : ''
const profile = release ? 'release' : 'debug'

// CI builds each component on an independent runner. Local callers can still stage the full set.
// Separate Cargo invocations preserve each binary's existing dependency features.
const binaries = [{ component: 'daemon', package: 'uc-daemon', name: 'uniclipd' }]
if (triple.includes('apple-darwin') || isWindows) {
  binaries.push({ component: 'helper', package: 'quick-panel', name: 'uniclip-quick-panel' })
}
const selected = binaries.filter(binary => component === 'all' || binary.component === component)
if (selected.length === 0)
  throw new Error(`Sidecar component ${component} is not supported on ${triple}`)

// Native Windows child processes cannot execute the shell Cargo entry from PATH.
const runCargoBuild = args => {
  const options = { cwd: repoRoot, stdio: 'inherit' }
  if (process.env.UC_ENGINE_MBX_BIN) {
    execFileSync(
      'bash',
      [join(repoRoot, 'scripts/build-cache/mbx.sh'), '--exec', 'cargo', ...args],
      options
    )
  } else {
    execFileSync('cargo', args, options)
  }
}

mkdirSync(stagingDir, { recursive: true })
for (const binary of selected) {
  const args = ['build', '--locked', '-p', binary.package, '--bin', binary.name]
  if (release) args.push('--release')
  if (target) args.push('--target', target)
  if (timings) args.push('--timings')
  console.log(`[sidecar] cargo ${args.join(' ')}`)
  runCargoBuild(args)

  const builtPath = target
    ? join(repoRoot, 'target', triple, profile, `${binary.name}${exeSuffix}`)
    : join(repoRoot, 'target', profile, `${binary.name}${exeSuffix}`)
  const sidecarPath = join(stagingDir, `${binary.name}-${triple}${exeSuffix}`)
  copyFileSync(builtPath, sidecarPath)
  if (!isWindows) chmodSync(sidecarPath, 0o755)
  console.log(`[sidecar] staged ${builtPath} -> ${sidecarPath}`)
}
