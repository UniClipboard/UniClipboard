// Development launcher for the Go GUI (Wails), the counterpart of `bun tauri:dev`.
//
//   bun wails:dev                      profile `dev`
//   bun wails:dev:profile <profile>    any other development profile
//
// It builds the daemon and the Go host, serves the shared React frontend with
// Vite (hot module replacement) and starts the app against that dev server.
// Frontend edits reload live; Go or daemon changes need a restart of this command.
import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import { availableDevPort, devServerPortForProfile, PROFILE_PATTERN } from './tauri-dev-profile.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const gui = path.join(root, 'apps/gui-go')
const out = path.join(root, 'target/gui-go/dev')

export const USAGE = `Usage: bun wails:dev
       bun wails:dev:profile <profile>

Runs the Go GUI in development mode with frontend hot reload. The profile
keeps data, keychain entries and the daemon separate from the production app.`

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', cwd: root, ...options })
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed (${result.status ?? result.error})`)
  }
}

// macOS only: the Wails notification service refuses to start without a bundle identifier, which a bare binary does not have.
// The development binary therefore runs from a minimal ad-hoc signed .app (the same Info.plist template as build.sh) with its own
// `.dev` identifier, so it never shares notification or login-item state with an installed app. It is still started as a plain
// child process, so stdio, the exit code and SIGTERM keep working.
export function makeDevBundle(binary) {
  const tauri = JSON.parse(fs.readFileSync(path.join(root, 'apps/gui/src-tauri/tauri.conf.json'), 'utf8'))
  const contents = path.join(path.dirname(binary), 'UniClipboardGoDev.app/Contents')
  fs.mkdirSync(path.join(contents, 'MacOS'), { recursive: true })
  const executable = path.join(contents, 'MacOS/gui-go')
  fs.copyFileSync(binary, executable)
  const plist = path.join(contents, 'Info.plist')
  fs.copyFileSync(path.join(gui, 'Info.plist'), plist)
  run('/usr/libexec/PlistBuddy', [
    '-c',
    `Set :CFBundleIdentifier ${tauri.identifier}.dev`,
    '-c',
    `Set :CFBundleName ${tauri.productName} Dev`,
    plist,
  ])
  run('codesign', ['--force', '--deep', '--sign', '-', path.join(contents, '..')])
  return executable
}

async function waitForServer(port, child, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error('Vite dev server exited before it was ready')
    try {
      const response = await fetch(`http://127.0.0.1:${port}/`)
      if (response.ok) return
    } catch {
      // not listening yet
    }
    await new Promise(resolve => setTimeout(resolve, 200))
  }
  throw new Error('Vite dev server did not become ready')
}

export async function main(argv, env = process.env) {
  let profile = env.UC_PROFILE
  if (argv[0] === '--help' || argv[0] === '-h') {
    console.log(USAGE)
    return 0
  }
  if (argv[0] === '--profile') {
    profile = argv[1]
    if (!profile) {
      console.error(`Profile name is required.\n\n${USAGE}`)
      return 1
    }
  }
  if (!profile || !PROFILE_PATTERN.test(profile)) {
    console.error(
      `Invalid profile name. Use 1-64 letters, numbers, underscores, or hyphens, starting with a letter or number.\n\n${USAGE}`
    )
    return 1
  }
  if (process.platform !== 'darwin') {
    console.error('The Go GUI currently runs only on macOS.')
    return 1
  }

  // An occupied profile port is replaced for this launch, as `tauri:dev:profile` does.
  const preferred = devServerPortForProfile(profile)
  let port
  try {
    port = await availableDevPort(preferred)
  } catch (error) {
    if (error.code !== 'EADDRINUSE') throw error
    port = await availableDevPort(0)
    console.log(`[wails-dev] port ${preferred} is occupied; using ${port}`)
  }

  console.log(`[wails-dev] profile=${profile} dev server port=${port}`)
  run('cargo', ['build', '-p', 'uc-daemon'])
  run('go', ['generate', './buildinfo'], { cwd: path.join(root, 'packages/desktop-host-go') })
  fs.mkdirSync(out, { recursive: true })
  fs.mkdirSync(path.join(gui, 'assets'), { recursive: true })
  fs.copyFileSync(
    path.join(root, 'apps/gui/src-tauri/icons/tray-icon@2x.png'),
    path.join(gui, 'assets/tray-icon@2x.png')
  )
  // The binary embeds frontend/dist; in dev the assets come from Vite, so an empty dist is enough.
  fs.mkdirSync(path.join(gui, 'frontend/dist'), { recursive: true })
  fs.writeFileSync(path.join(gui, 'frontend/dist/.gitkeep'), '')
  // No `production` tag: Wails then proxies assets to FRONTEND_DEVSERVER_URL.
  const built = path.join(out, 'gui-go-dev')
  run('go', ['build', '-o', built, '.'], { cwd: gui })
  const binary = makeDevBundle(built)

  const childEnv = {
    ...env,
    UNICLIPBOARD_ENV: 'development',
    UC_PROFILE: profile,
    UC_DEV_SERVER_PORT: String(port),
    // The host resolves the daemon next to itself or on PATH.
    PATH: `${path.join(root, 'target/debug')}${path.delimiter}${env.PATH ?? ''}`,
  }
  const vite = spawn('bun', ['--bun', 'run', '--cwd', 'apps/gui-go', 'dev'], {
    cwd: root,
    env: childEnv,
    stdio: 'inherit',
  })
  let app
  const stop = () => {
    vite.kill('SIGTERM')
    app?.kill('SIGTERM')
  }
  process.on('SIGINT', stop)
  process.on('SIGTERM', stop)
  try {
    await waitForServer(port, vite)
  } catch (error) {
    stop()
    console.error(error.message)
    return 1
  }
  app = spawn(binary, [], {
    cwd: root,
    env: { ...childEnv, FRONTEND_DEVSERVER_URL: `http://127.0.0.1:${port}` },
    stdio: 'inherit',
  })
  const code = await new Promise(resolve =>
    app.on('exit', (status, signal) => resolve(status ?? (signal ? 1 : 0)))
  )
  vite.kill('SIGTERM')
  return code
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  process.exitCode = await main(process.argv.slice(2))
}
