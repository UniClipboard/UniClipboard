#!/usr/bin/env node
// Local updater test harness — exercise the full update -> restart flow on the
// dev machine without publishing two real releases.
//
// Why this exists: bugs in the self-update path (e.g. the post-update daemon
// instance-lock eviction in #1063) can only be verified by actually driving the
// updater: download a "newer" artifact, verify its signature, install it, and
// restart. Normally that needs two published releases on a real update channel.
// This harness reduces it to two *local* debug builds plus a localhost server.
//
// How it works:
//   - A throwaway minisign keypair signs the local artifact (`keygen`). The
//     running app verifies against it via the debug-only `UC_UPDATE_PUBKEY`
//     override; signature verification stays ON, so this drives the real path.
//   - The running app is redirected to a localhost manifest via the debug-only
//     `UC_UPDATE_ENDPOINT` override (see `do_check_for_update` /
//     `apply_dev_updater_override` in `commands/updater.rs`). Both overrides are
//     compiled out of release builds, so they cannot ship.
//
// Typical loop (each command in the order below; `serve` and `run` are
// long-running, so keep them in separate terminals):
//   node scripts/dev-update-loop.mjs keygen          # one-time
//   node scripts/dev-update-loop.mjs build-base      # build + stage the "old" app
//   node scripts/dev-update-loop.mjs build-update    # build the "newer" artifact + manifest
//   node scripts/dev-update-loop.mjs serve           # terminal A: serve manifest + artifact
//   node scripts/dev-update-loop.mjs run             # terminal B: launch the old app w/ overrides
// Then trigger an update check in the app and watch it download -> install ->
// restart, with the new daemon evicting the old one.
//
// macOS only for now (the dev machine is darwin); the artifact format and
// install path differ on Windows/Linux. See docs/development/local-update-loop.md.

import { spawnSync } from 'node:child_process'
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { createServer } from 'node:http'
import { homedir } from 'node:os'
import { basename, dirname, extname, join, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const srcTauri = join(repoRoot, 'src-tauri')
const tauriConf = join(srcTauri, 'tauri.conf.json')

// All generated state lives under the gitignored target dir.
const workDir = join(repoRoot, 'target', 'dev-update')
const runDir = join(workDir, 'run') // holds the running ("old") .app bundle
const serveDir = join(workDir, 'serve') // holds the manifest + update artifact
const stateFile = join(workDir, 'state.json')

// Keys live outside target/ so `cargo clean` does not wipe them.
const keyDir = process.env.UC_DEV_UPDATER_KEY_DIR || join(homedir(), '.uniclip-dev-updater')
const privKeyFile = join(keyDir, 'dev.key')
const pubKeyFile = join(keyDir, 'dev.key.pub')
const passwordFile = join(keyDir, 'password.txt')

// Isolated identity so the test build never touches the real install's data.
const IDENTIFIER = 'app.uniclipboard.desktop.dev'
const PRODUCT_NAME = 'UniClipboard Dev'
const RUN_PROFILE = process.env.UC_DEV_UPDATER_PROFILE || 'updtest'
const MANIFEST_NAME = 'update.json'
const DEFAULT_PORT = Number(process.env.UC_DEV_UPDATER_PORT || 8723)

// Native debug bundle output (no --target → not under a triple subdir).
const bundleMacosDir = join(repoRoot, 'target', 'debug', 'bundle', 'macos')

function die(msg) {
  process.stderr.write(`error: ${msg}\n`)
  process.exit(1)
}

function log(msg) {
  process.stderr.write(`${msg}\n`)
}

function ensureDarwin() {
  if (process.platform !== 'darwin') {
    die(`this harness currently supports macOS only (got ${process.platform})`)
  }
}

function readJson(path, fallback) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    return fallback
  }
}

function readState() {
  return readJson(stateFile, {})
}

function writeState(next) {
  mkdirSync(workDir, { recursive: true })
  writeFileSync(stateFile, JSON.stringify(next, null, 2) + '\n', 'utf8')
}

function currentVersion() {
  const conf = readJson(tauriConf, null)
  if (!conf?.version) die(`could not read version from ${tauriConf}`)
  return conf.version
}

// Strictly-increasing bump. Increments a numeric prerelease suffix
// (`-alpha.4` → `-alpha.5`) when present, otherwise bumps the patch.
function bumpVersion(version) {
  const pre = version.match(/^(.*-(?:alpha|beta|rc)\.)(\d+)$/)
  if (pre) return `${pre[1]}${Number(pre[2]) + 1}`
  const core = version.match(/^(\d+)\.(\d+)\.(\d+)/)
  if (!core) die(`cannot bump unrecognized version: ${version}`)
  return `${core[1]}.${core[2]}.${Number(core[3]) + 1}`
}

function run(cmd, args, opts = {}) {
  log(`$ ${cmd} ${args.join(' ')}`)
  const res = spawnSync(cmd, args, { stdio: 'inherit', cwd: repoRoot, ...opts })
  if (res.status !== 0) {
    die(`${cmd} exited with ${res.status ?? res.signal}`)
  }
}

function parseFlags(argv) {
  const flags = {}
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a.startsWith('--')) {
      const eq = a.indexOf('=')
      if (eq !== -1) {
        flags[a.slice(2, eq)] = a.slice(eq + 1)
      } else if (argv[i + 1] && !argv[i + 1].startsWith('--')) {
        flags[a.slice(2)] = argv[++i]
      } else {
        flags[a.slice(2)] = true
      }
    }
  }
  return flags
}

// Stage the `uniclipd` daemon sidecar (debug) unless already present.
function ensureSidecar(flags) {
  const triple = spawnSync('rustc', ['--print', 'host-tuple'], {
    cwd: repoRoot,
    encoding: 'utf8',
  }).stdout?.trim()
  const staged = triple && existsSync(join(srcTauri, 'binaries', `uniclipd-${triple}`))
  if (staged && !flags['rebuild-sidecar']) {
    log(`sidecar already staged for ${triple} (pass --rebuild-sidecar to force)`)
    return
  }
  run('node', [join(repoRoot, 'scripts', 'prepare-daemon-sidecar.mjs'), '--debug'])
}

// Write a minimal `-c` override config merged onto tauri.conf.json: isolated
// identity + the requested version. Returns the temp config path.
function writeOverrideConfig(version) {
  mkdirSync(workDir, { recursive: true })
  const path = join(workDir, 'tauri.override.conf.json')
  const override = {
    productName: PRODUCT_NAME,
    identifier: IDENTIFIER,
    version,
  }
  writeFileSync(path, JSON.stringify(override, null, 2) + '\n', 'utf8')
  return path
}

function signingEnv() {
  if (!existsSync(privKeyFile)) {
    die(
      `dev signing key not found at ${privKeyFile} — run: node scripts/dev-update-loop.mjs keygen`
    )
  }
  return {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(privKeyFile, 'utf8').trim(),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: existsSync(passwordFile)
      ? readFileSync(passwordFile, 'utf8')
      : '',
  }
}

function tauriBuild(version, env) {
  const overrideConf = writeOverrideConfig(version)
  // `bun x tauri` forwards all args verbatim to the local tauri CLI.
  // --bundles app keeps it to the .app + updater artifact (skips the slow dmg).
  run('bun', ['x', 'tauri', 'build', '--debug', '--bundles', 'app', '-c', overrideConf], { env })
}

function findNewest(dir, predicate) {
  if (!existsSync(dir)) return null
  const matches = readdirSync(dir)
    .filter(predicate)
    .map(name => ({ name, mtime: statSync(join(dir, name)).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime)
  return matches[0] ? join(dir, matches[0].name) : null
}

// ── commands ────────────────────────────────────────────────────────────────

function cmdKeygen(flags) {
  if (existsSync(privKeyFile) && !flags.force) {
    log(`dev key already exists at ${privKeyFile} (pass --force to regenerate)`)
    return
  }
  mkdirSync(keyDir, { recursive: true })
  const password = process.env.UC_DEV_UPDATER_KEY_PASSWORD ?? ''
  writeFileSync(passwordFile, password, 'utf8')
  run('bun', ['x', 'tauri', 'signer', 'generate', '-w', privKeyFile, '-p', password, '-f', '--ci'])
  log(`\ndev keypair written to ${keyDir}`)
  log(`public key (for reference): ${pubKeyFile}`)
}

function cmdBuildBase(flags) {
  ensureDarwin()
  ensureSidecar(flags)
  const version = currentVersion()
  log(`building base ("old") app at version ${version}`)
  tauriBuild(version, signingEnv())

  const appBundle = findNewest(bundleMacosDir, n => n.endsWith('.app'))
  if (!appBundle) die(`no .app bundle found in ${bundleMacosDir}`)

  rmSync(runDir, { recursive: true, force: true })
  mkdirSync(runDir, { recursive: true })
  const dest = join(runDir, basename(appBundle))
  cpSync(appBundle, dest, { recursive: true })

  // Reset the version cursor so the next build-update bumps from the base.
  writeState({ ...readState(), runningVersion: version, lastUpdateVersion: version })
  log(`\nbase app staged at: ${dest}`)
  log(`next: build-update, then serve + run`)
}

function cmdBuildUpdate(flags) {
  ensureDarwin()
  ensureSidecar(flags)
  const state = readState()
  const base = state.lastUpdateVersion || currentVersion()
  const version = flags['update-version'] || bumpVersion(base)
  log(`building update ("new") artifact at version ${version}`)
  tauriBuild(version, signingEnv())

  const artifact = findNewest(bundleMacosDir, n => n.endsWith('.app.tar.gz'))
  if (!artifact) {
    die(
      `no .app.tar.gz updater artifact found in ${bundleMacosDir} (is createUpdaterArtifacts on?)`
    )
  }
  const sig = `${artifact}.sig`
  if (!existsSync(sig)) die(`missing signature next to artifact: ${sig}`)

  // Clean, space-free filename for the served URL (bytes unchanged → sig valid).
  const arch = process.arch === 'arm64' ? 'aarch64' : 'x86_64'
  const artifactName = `UniClipboardDev_${version}_${arch}.app.tar.gz`

  rmSync(serveDir, { recursive: true, force: true })
  mkdirSync(serveDir, { recursive: true })
  cpSync(artifact, join(serveDir, artifactName))

  const port = Number(flags.port || DEFAULT_PORT)
  // Manifest shape mirrors scripts/assemble-update-manifest.js (the canonical
  // release manifest). `signature` is the raw .sig content (already base64).
  const manifest = {
    version,
    notes: 'Local dev update test build.',
    pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    platforms: {
      [`darwin-${arch}`]: {
        signature: readFileSync(sig, 'utf8').trim(),
        url: `http://localhost:${port}/${artifactName}`,
      },
    },
  }
  writeFileSync(join(serveDir, MANIFEST_NAME), JSON.stringify(manifest, null, 2) + '\n', 'utf8')

  writeState({ ...state, lastUpdateVersion: version })
  log(`\nupdate artifact + manifest staged in: ${serveDir}`)
  log(`manifest version ${version} → served at http://localhost:${port}/${MANIFEST_NAME}`)
  log(`next: serve (terminal A) + run (terminal B)`)
}

function cmdServe(flags) {
  if (!existsSync(join(serveDir, MANIFEST_NAME))) {
    die(`no manifest in ${serveDir} — run build-update first`)
  }
  const port = Number(flags.port || DEFAULT_PORT)
  const server = createServer((req, res) => {
    const name =
      decodeURIComponent((req.url || '/').split('?')[0].replace(/^\//, '')) || MANIFEST_NAME
    const filePath = join(serveDir, name)
    // Refuse path traversal; only serve files that live directly in serveDir.
    if (dirname(filePath) !== serveDir || !existsSync(filePath)) {
      res.writeHead(404)
      res.end('not found')
      log(`404 ${req.method} ${req.url}`)
      return
    }
    const isJson = extname(filePath) === '.json'
    res.writeHead(200, {
      'content-type': isJson ? 'application/json' : 'application/octet-stream',
      'content-length': statSync(filePath).size,
    })
    res.end(readFileSync(filePath))
    log(`200 ${req.method} ${req.url}`)
  })
  server.listen(port, '127.0.0.1', () => {
    log(`serving ${serveDir} at http://localhost:${port}/`)
    log(`manifest: http://localhost:${port}/${MANIFEST_NAME}`)
    log(`(Ctrl+C to stop)`)
  })
}

function pubkeyOverride() {
  if (!existsSync(pubKeyFile)) die(`public key not found at ${pubKeyFile} — run keygen`)
  // The .pub file content is ALREADY the base64 blob the plugin expects: it
  // base64-decodes config.pubkey once (verify_signature in
  // tauri-plugin-updater) to recover the minisign "untrusted comment…" text.
  // This is the exact same string format as tauri.conf.json's pubkey, so pass
  // the file content verbatim — do NOT re-encode it.
  return readFileSync(pubKeyFile, 'utf8').trim()
}

function runEnv(port) {
  return {
    UC_UPDATE_ENDPOINT: `http://localhost:${port}/${MANIFEST_NAME}`,
    UC_UPDATE_PUBKEY: pubkeyOverride(),
    UC_PROFILE: RUN_PROFILE,
  }
}

function cmdRun(flags) {
  ensureDarwin()
  const appBundle = findNewest(runDir, n => n.endsWith('.app'))
  if (!appBundle) die(`no base app in ${runDir} — run build-base first`)
  const macosDir = join(appBundle, 'Contents', 'MacOS')
  // Contents/MacOS holds BOTH the GUI binary and the bundled `uniclipd`
  // sidecar, so pick the GUI binary via the bundle's CFBundleExecutable
  // rather than guessing by directory order (`defaults` reads XML + binary
  // plists). Fall back to the only non-sidecar binary if that lookup fails.
  const fromPlist = spawnSync(
    'defaults',
    ['read', join(appBundle, 'Contents', 'Info'), 'CFBundleExecutable'],
    { encoding: 'utf8' }
  ).stdout?.trim()
  const exeName =
    fromPlist || readdirSync(macosDir).find(n => !n.startsWith('.') && n !== 'uniclipd')
  if (!exeName) die(`could not resolve the GUI binary in ${macosDir}`)
  const exe = join(macosDir, exeName)
  const port = Number(flags.port || DEFAULT_PORT)
  // The macOS updater extracts into a tempfile dir (env::temp_dir → $TMPDIR)
  // and `rename`s it over the app bundle. rename across volumes fails with
  // EXDEV, which happens when the repo (hence the run app) lives on a
  // non-boot volume while $TMPDIR defaults to /var/folders on the boot
  // volume. Co-locate $TMPDIR with the run app so the rename stays
  // intra-volume — mirroring production, where /Applications and $TMPDIR
  // share the boot volume.
  const tmpDir = join(workDir, 'tmp')
  mkdirSync(tmpDir, { recursive: true })
  const env = { ...process.env, ...runEnv(port), TMPDIR: tmpDir }
  log(`launching ${exe}`)
  log(`  UC_UPDATE_ENDPOINT=${env.UC_UPDATE_ENDPOINT}`)
  log(`  UC_PROFILE=${env.UC_PROFILE}`)
  log(`  TMPDIR=${env.TMPDIR}`)
  // Launch the inner binary (not `open`) so override env vars apply and logs
  // stream to this terminal. app.restart() after install re-execs this binary
  // with the same env, so the override survives the restart.
  run(exe, [], { env })
}

function cmdInfo() {
  const state = readState()
  const port = DEFAULT_PORT
  log(`repo:            ${repoRoot}`)
  log(`current version: ${currentVersion()}`)
  log(`last update ver: ${state.lastUpdateVersion || '(none)'}`)
  log(
    `key dir:         ${keyDir} (${existsSync(privKeyFile) ? 'present' : 'MISSING — run keygen'})`
  )
  log(`run dir:         ${runDir}`)
  log(`serve dir:       ${serveDir}`)
  log(`profile:         ${RUN_PROFILE}`)
  log(`default port:    ${port}`)
  if (existsSync(pubKeyFile)) {
    log(`\nrun-time overrides:`)
    log(`  UC_UPDATE_ENDPOINT=http://localhost:${port}/${MANIFEST_NAME}`)
    log(`  UC_UPDATE_PUBKEY=${pubkeyOverride().slice(0, 24)}…`)
  }
}

function cmdHelp() {
  process.stdout.write(`Local updater test harness (macOS)

Usage: node scripts/dev-update-loop.mjs <command> [options]

Commands:
  keygen          Generate the throwaway dev minisign keypair (one-time).
                    --force                regenerate even if it exists
  build-base      Build the "old" .app at the committed version, stage it to run.
  build-update    Build the "new" updater artifact + manifest at a bumped version.
                    --update-version <v>   explicit version (default: auto-bump)
                    --port <n>             port baked into the manifest URL
  serve           Serve the manifest + artifact over http://localhost:<port>.
                    --port <n>             default ${DEFAULT_PORT}
  run             Launch the staged "old" app with the dev updater overrides.
                    --port <n>             must match the serve port
  info            Print resolved paths, versions, and the override env vars.

Shared options:
  --rebuild-sidecar   force-rebuild the uniclipd daemon sidecar before a build

Env overrides:
  UC_DEV_UPDATER_KEY_DIR        key location (default ~/.uniclip-dev-updater)
  UC_DEV_UPDATER_KEY_PASSWORD   key password (default empty)
  UC_DEV_UPDATER_PROFILE        UC_PROFILE for the run (default ${RUN_PROFILE})
  UC_DEV_UPDATER_PORT           default port (default ${DEFAULT_PORT})

See docs/development/local-update-loop.md for the full runbook.
`)
}

const commands = {
  keygen: cmdKeygen,
  'build-base': cmdBuildBase,
  'build-update': cmdBuildUpdate,
  serve: cmdServe,
  run: cmdRun,
  info: cmdInfo,
  help: cmdHelp,
}

const [command, ...rest] = process.argv.slice(2)
const handler = commands[command]
if (!handler) {
  if (command) process.stderr.write(`unknown command: ${command}\n\n`)
  cmdHelp()
  process.exit(command ? 1 : 0)
}
handler(parseFlags(rest))
