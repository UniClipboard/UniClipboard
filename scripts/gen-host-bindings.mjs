#!/usr/bin/env node

// Generates the host command bindings with the official Wails generator (`wails3 generate bindings`)
// and the error catalog form with apps/gui-go/cmd/hostcontract.
//
//   node scripts/gen-host-bindings.mjs            write apps/gui-go/frontend/bindings and the error catalog
//   node scripts/gen-host-bindings.mjs --check    regenerate into a scratch directory and fail on any difference
//
// The Go methods of HostService (apps/gui-go) are the contract. The generator version is the Wails version
// pinned in apps/gui-go/go.mod, so there is no second pin. Environment:
//   UC_BINDINGS_GOOS / UC_BINDINGS_TAGS   cross analysis (the output must not depend on the OS; CI checks Linux)

import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const GUI_GO = join(ROOT, 'apps/gui-go')
const OUT = join(GUI_GO, 'frontend/bindings')
const check = process.argv.includes('--check')

const goMod = readFileSync(join(GUI_GO, 'go.mod'), 'utf8')
const wails = /github\.com\/wailsapp\/wails\/v3 (v\S+)/.exec(goMod)?.[1]
const toolchain = /^toolchain (go\S+)/m.exec(goMod)?.[1]
if (!wails) throw new Error('the Wails version is not pinned in apps/gui-go/go.mod')

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options })
  if (result.status !== 0) {
    console.error(`gen-host-bindings: \`${command} ${args.join(' ')}\` failed (${result.status})`)
    process.exit(result.status ?? 1)
  }
}

// The generator is built for the machine it runs on, before GOOS is applied: GOOS/GOARCH only select which
// source files of apps/gui-go it analyses.
let generatorBinary
function generatorPath() {
  if (generatorBinary) return generatorBinary
  const binDir = mkdtempSync(join(tmpdir(), 'uc-wails3-'))
  const env = { ...process.env, GOBIN: binDir }
  // The analysis must use the toolchain the module builds with: a generator compiled by an older Go
  // reports warnings for newer standard library sources and may type them wrongly.
  if (toolchain) env.GOTOOLCHAIN = toolchain
  // On Linux the generator links the Wails GTK backend through cgo; the host is built with the gtk3 tag (the
  // build image carries GTK3, not the GTK4 default), so the generator must be too.
  const tags = process.platform === 'linux' ? ['-tags', 'gtk3'] : []
  run('go', ['install', ...tags, `github.com/wailsapp/wails/v3/cmd/wails3@${wails}`], {
    cwd: tmpdir(),
    env,
  })
  generatorBinary = join(binDir, 'wails3')
  process.on('exit', () => rmSync(binDir, { recursive: true, force: true }))
  return generatorBinary
}

function generate(dir) {
  const bin = generatorPath()
  const env = { ...process.env }
  if (toolchain) env.GOTOOLCHAIN = toolchain
  if (process.env.UC_BINDINGS_GOOS) {
    env.GOOS = process.env.UC_BINDINGS_GOOS
    env.CGO_ENABLED = '0'
  }
  // The directory is emptied here rather than by the generator's -clean, which moves the old output to the OS trash.
  rmSync(dir, { recursive: true, force: true })
  const args = ['generate', 'bindings', '-ts', '-i', '-clean=false', '-d', dir]
  if (process.env.UC_BINDINGS_TAGS) args.push('-f', `-tags ${process.env.UC_BINDINGS_TAGS}`)
  args.push('.')
  run(bin, args, { cwd: GUI_GO, env })
}

function listFiles(base, dir = base) {
  const out = []
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    if (statSync(path).isDirectory()) out.push(...listFiles(base, path))
    else out.push(relative(base, path))
  }
  return out.sort()
}

if (!check) {
  generate(OUT)
  run('go', ['run', './cmd/hostcontract', 'errors-ts'], { cwd: GUI_GO })
  process.exit(0)
}

const scratchRoot = mkdtempSync(join(tmpdir(), 'uc-host-bindings-'))
const scratch = join(scratchRoot, 'bindings')
try {
  generate(scratch)
  const want = listFiles(scratch)
  const have = existsSync(OUT) ? listFiles(OUT) : []
  const problems = []
  for (const file of new Set([...want, ...have])) {
    if (!want.includes(file)) problems.push(`stale file: ${file}`)
    else if (!have.includes(file)) problems.push(`missing file: ${file}`)
    else if (readFileSync(join(scratch, file), 'utf8') !== readFileSync(join(OUT, file), 'utf8'))
      problems.push(`changed: ${file}`)
  }
  run('go', ['run', './cmd/hostcontract', 'errors-ts', '-check'], { cwd: GUI_GO })
  if (problems.length > 0) {
    console.error(problems.join('\n'))
    console.error(
      'Host bindings are stale: run `bun run gen:host-contract` and commit apps/gui-go/frontend/bindings.'
    )
    process.exit(1)
  }
  console.log(`host bindings are up to date (${want.length} files, Wails ${wails})`)
} finally {
  rmSync(scratchRoot, { recursive: true, force: true })
}
