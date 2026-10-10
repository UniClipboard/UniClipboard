#!/usr/bin/env node
// Build the `uniclipd` daemon for a target triple and stage it, under the name
// `uniclipd-<target-triple>`, in `target/sidecar-staging/`. On macOS it also stages the native
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
//   node scripts/stage-daemon.mjs [--target <triple>] [--debug] [--timings]

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
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if (arg === '--target') {
      target = argv[++i] ?? ''
    } else if (arg.startsWith('--target=')) {
      target = arg.slice('--target='.length)
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
  return { target, release, timings }
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

const { target, release, timings } = parseArgs(process.argv.slice(2))
const triple = target || hostTriple()
const isWindows = triple.includes('windows')
const exeSuffix = isWindows ? '.exe' : ''
const profile = release ? 'release' : 'debug'

// 1) Build the daemon binary for the requested target.
const buildArgs = ['build', '--locked', '-p', 'uc-daemon', '--bin', 'uniclipd']
if (release) buildArgs.push('--release')
if (target) buildArgs.push('--target', target)
if (timings) buildArgs.push('--timings')
console.log(`[sidecar] cargo ${buildArgs.join(' ')}`)
execFileSync('cargo', buildArgs, { cwd: repoRoot, stdio: 'inherit' })

// 2) Locate the compiled binary. With `--target` cargo nests the output under
//    the triple; a native build (no `--target`) lands in target/<profile>/.
const builtPath = target
  ? join(repoRoot, 'target', triple, profile, `uniclipd${exeSuffix}`)
  : join(repoRoot, 'target', profile, `uniclipd${exeSuffix}`)

// 3) Stage it as `uniclipd-<triple>`.
const binariesDir = stagingDir
mkdirSync(binariesDir, { recursive: true })
const sidecarPath = join(binariesDir, `uniclipd-${triple}${exeSuffix}`)
copyFileSync(builtPath, sidecarPath)
if (!isWindows) chmodSync(sidecarPath, 0o755)
console.log(`[sidecar] staged ${builtPath} -> ${sidecarPath}`)

// 4) macOS only: the native quick panel helper ships next to the app executable too, where
//    the Go host's `quickpanelhelper.ResolveExePath` looks for it. Other platforms keep the WebView panel.
if (triple.includes('apple-darwin')) {
  const helperArgs = ['build', '--locked', '-p', 'quick-panel', '--bin', 'uniclip-quick-panel']
  if (release) helperArgs.push('--release')
  if (target) helperArgs.push('--target', target)
  if (timings) helperArgs.push('--timings')
  console.log(`[sidecar] cargo ${helperArgs.join(' ')}`)
  execFileSync('cargo', helperArgs, { cwd: repoRoot, stdio: 'inherit' })
  const helperBuilt = target
    ? join(repoRoot, 'target', triple, profile, 'uniclip-quick-panel')
    : join(repoRoot, 'target', profile, 'uniclip-quick-panel')
  const helperPath = join(binariesDir, `uniclip-quick-panel-${triple}`)
  copyFileSync(helperBuilt, helperPath)
  chmodSync(helperPath, 0o755)
  console.log(`[sidecar] staged ${helperBuilt} -> ${helperPath}`)
}
