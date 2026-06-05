#!/usr/bin/env node
// Prepare the `uniclipd` daemon as a Tauri sidecar (externalBin).
//
// ADR-008 D13 bundles `uniclipd` into the GUI installer so the GUI (and CLI)
// can spawn it as a *sibling* of the app executable — see
// `uc-daemon-local` `spawn.rs::resolve_daemon_exe_path`, whose first strategy
// is "look for `uniclipd` next to the current exe". Tauri's externalBin
// mechanism copies `src-tauri/binaries/uniclipd-<target-triple>` into the
// bundle next to the main binary (Contents/MacOS on macOS, usr/bin on Linux,
// install dir on Windows) with the triple suffix stripped, which lands exactly
// where the sibling lookup expects it.
//
// This script builds the daemon for a given target and stages it under that
// exact name. It is invoked:
//   - by CI (build.yml / alpha-build.yml) right before `tauri build`, passing
//     the same `--target <triple>` the GUI build uses (matrix.args) so the
//     staged sidecar name matches what tauri-cli looks up;
//   - by `bun run daemon:dev` (debug) and local `tauri:build:dev` (release)
//     for a native build.
//
// `tauri build`/`tauri dev` hard-fail if the expected sidecar file is missing,
// so this must complete before either runs. snap/AUR packaging bypasses
// tauri-cli (plain `cargo build`) and therefore installs `uniclipd` directly
// instead of going through this script.
//
// Usage:
//   node scripts/prepare-daemon-sidecar.mjs [--target <triple>] [--debug]

import { execFileSync } from 'node:child_process'
import { chmodSync, copyFileSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const srcTauri = join(repoRoot, 'src-tauri')

function parseArgs(argv) {
  let target = ''
  let release = true
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
    }
    // Unknown args are ignored on purpose so callers can forward the GUI
    // build's `${{ matrix.args }}` verbatim (it is either `--target <triple>`
    // or empty).
  }
  return { target, release }
}

function hostTriple() {
  // rustc 1.84+ prints the canonical host tuple directly; older toolchains
  // need the verbose-version `host:` line parsed out. Run inside src-tauri so
  // the pinned toolchain's rustc answers.
  try {
    return execFileSync('rustc', ['--print', 'host-tuple'], {
      cwd: srcTauri,
      encoding: 'utf8',
    }).trim()
  } catch {
    const verbose = execFileSync('rustc', ['-Vv'], {
      cwd: srcTauri,
      encoding: 'utf8',
    })
    const match = verbose.match(/^host:\s*(.+)$/m)
    if (!match) {
      throw new Error('could not determine host target triple from `rustc -Vv`')
    }
    return match[1].trim()
  }
}

const { target, release } = parseArgs(process.argv.slice(2))
const triple = target || hostTriple()
const isWindows = triple.includes('windows')
const exeSuffix = isWindows ? '.exe' : ''
const profile = release ? 'release' : 'debug'

// 1) Build the daemon binary for the requested target.
const buildArgs = ['build', '-p', 'uc-daemon', '--bin', 'uniclipd']
if (release) buildArgs.push('--release')
if (target) buildArgs.push('--target', target)
console.log(`[sidecar] cargo ${buildArgs.join(' ')}`)
execFileSync('cargo', buildArgs, { cwd: srcTauri, stdio: 'inherit' })

// 2) Locate the compiled binary. With `--target` cargo nests the output under
//    the triple; a native build (no `--target`) lands in target/<profile>/.
const builtPath = target
  ? join(srcTauri, 'target', triple, profile, `uniclipd${exeSuffix}`)
  : join(srcTauri, 'target', profile, `uniclipd${exeSuffix}`)

// 3) Stage it under the Tauri sidecar name `uniclipd-<triple>`.
const binariesDir = join(srcTauri, 'binaries')
mkdirSync(binariesDir, { recursive: true })
const sidecarPath = join(binariesDir, `uniclipd-${triple}${exeSuffix}`)
copyFileSync(builtPath, sidecarPath)
if (!isWindows) chmodSync(sidecarPath, 0o755)
console.log(`[sidecar] staged ${builtPath} -> ${sidecarPath}`)
