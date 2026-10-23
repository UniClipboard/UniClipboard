// Development launcher for the Go GUI (Wails).
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
import { availableDevPort, devServerPortForProfile, PROFILE_PATTERN } from './dev-profile.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const gui = path.join(root, 'apps/gui-go')
const out = path.join(root, 'target/gui-go/dev')

export const USAGE = `Usage: bun wails:dev
       bun wails:dev:profile <profile>
       add --no-helper to leave the native quick panel helper out (macOS)
       Linux requires GTK >= 4.10 and WebKitGTK 6.0

Runs the Go GUI in development mode with frontend hot reload. The profile
keeps data, keychain entries and the daemon separate from the production app.`

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    cwd: root,
    windowsHide: true,
    ...options,
  })
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed (${result.status ?? result.error})`)
  }
}

export function linuxDesktopEnvironment(env) {
  if (env.DISPLAY || env.WAYLAND_DISPLAY) return env
  const result = spawnSync('systemctl', ['--user', 'show-environment', '--output=json'], {
    env,
    encoding: 'utf8',
    timeout: 5000,
  })
  if (result.error || result.status !== 0) return env
  let session
  try {
    session = JSON.parse(result.stdout)
  } catch {
    return env
  }
  if (!session.DISPLAY && !session.WAYLAND_DISPLAY) return env
  const desktopEnv = { ...env }
  // Import only desktop connection settings, never the manager's PATH or app settings.
  for (const name of [
    'DISPLAY',
    'WAYLAND_DISPLAY',
    'XDG_RUNTIME_DIR',
    'XDG_SESSION_TYPE',
    'DBUS_SESSION_BUS_ADDRESS',
    'XAUTHORITY',
    'HYPRLAND_INSTANCE_SIGNATURE',
    'NIRI_SOCKET',
    'XDG_CURRENT_DESKTOP',
  ]) {
    if (typeof session[name] === 'string') desktopEnv[name] = session[name]
  }
  return desktopEnv
}

// macOS only: the Wails notification service refuses to start without a bundle identifier, which a bare binary does not have.
// The development binary therefore runs from a minimal ad-hoc signed .app (the same Info.plist template as build.sh) with its own
// `.dev` identifier, so it never shares notification or login-item state with an installed app. It is still started as a plain
// child process, so stdio, the exit code and SIGTERM keep working.
// The native quick panel helper goes next to the host executable, where the host looks for it: without it the host falls back
// to the WebView quick panel, which has no modifier double-tap trigger on macOS. Without `helper` the bundle carries none,
// and one left by an earlier launch is removed.
export function makeDevBundle(binary, helper) {
  const app = JSON.parse(fs.readFileSync(path.join(root, 'apps/gui-go/app.json'), 'utf8'))
  const contents = path.join(path.dirname(binary), 'UniClipboardGoDev.app/Contents')
  fs.mkdirSync(path.join(contents, 'MacOS'), { recursive: true })
  fs.mkdirSync(path.join(contents, 'Resources'), { recursive: true })
  fs.copyFileSync(path.join(gui, 'icons/icon.icns'), path.join(contents, 'Resources/icon.icns'))
  const executable = path.join(contents, 'MacOS/gui-go')
  fs.copyFileSync(binary, executable)
  const bundledHelper = path.join(contents, 'MacOS/uniclip-quick-panel')
  if (helper) {
    fs.copyFileSync(helper, bundledHelper)
    fs.chmodSync(bundledHelper, 0o755)
  } else {
    fs.rmSync(bundledHelper, { force: true })
  }
  const plist = path.join(contents, 'Info.plist')
  fs.copyFileSync(path.join(gui, 'Info.plist'), plist)
  run('/usr/libexec/PlistBuddy', [
    '-c',
    `Set :CFBundleIdentifier ${app.identifier}.dev`,
    '-c',
    `Set :CFBundleName ${app.productName} Dev`,
    '-c',
    'Add :CFBundleIconFile string icon',
    plist,
  ])
  run('python3', [path.join(gui, 'packaging/macos/ui_bundle.py'), path.join(contents, '..')])
  run('codesign', ['--force', '--deep', '--sign', '-', path.join(contents, '..')])
  return executable
}

async function waitForServer(port, child, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error('Vite dev server exited before it was ready')
    }
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

export async function main(args, env = process.env) {
  const withHelper = !args.includes('--no-helper')
  const explicitGtk4 = args.includes('--gtk4')
  const argv = args.filter(arg => arg !== '--no-helper' && arg !== '--gtk4')
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
  if (!['darwin', 'win32', 'linux'].includes(process.platform)) {
    console.error('The development launcher currently supports macOS, Windows and Linux.')
    return 1
  }
  if (explicitGtk4 && process.platform !== 'linux') {
    console.error('--gtk4 is supported only on Linux.')
    return 1
  }

  if (process.platform === 'linux') {
    env = linuxDesktopEnvironment(env)
    if (!env.DISPLAY && !env.WAYLAND_DISPLAY) {
      console.error(
        'No graphical desktop session found. Run this command from a desktop terminal with DISPLAY or WAYLAND_DISPLAY set.'
      )
      return 1
    }
    const dependencies = spawnSync('pkg-config', ['--exists', 'gtk4 >= 4.10', 'webkitgtk-6.0'], {
      env,
      encoding: 'utf8',
    })
    if (dependencies.error || dependencies.status !== 0) {
      console.error('Linux development requires GTK >= 4.10 and WebKitGTK 6.0 development packages.')
      return 1
    }
  }

  const goVersion = spawnSync('go', ['version'], { env, encoding: 'utf8', windowsHide: true })
  if (goVersion.error || goVersion.status !== 0) {
    console.error(
      'Go is required to build the development GUI. Install Go and ensure go is on PATH.'
    )
    return 1
  }

  // An occupied profile port is replaced for this launch.
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
  // macOS ships the native quick panel helper next to the app (stage-daemon.mjs), so the dev bundle carries it too.
  if (process.platform === 'darwin' && withHelper) {
    run('cargo', ['build', '--locked', '-p', 'quick-panel', '--bin', 'uniclip-quick-panel'])
  }
  run('go', ['generate', './buildinfo'], { cwd: path.join(root, 'packages/desktop-host-go') })
  fs.mkdirSync(out, { recursive: true })
  // The binary embeds frontend/dist; in dev the assets come from Vite, so an empty dist is enough.
  fs.mkdirSync(path.join(gui, 'frontend/dist'), { recursive: true })
  fs.writeFileSync(path.join(gui, 'frontend/dist/.gitkeep'), '')
  // No `production` tag: Wails then proxies assets to FRONTEND_DEVSERVER_URL. On macOS the
  // `private_mac_apis` tag lets the window material show through the page, as in the packaged app.
  const built = path.join(out, process.platform === 'win32' ? 'gui-go-dev.exe' : 'gui-go-dev')
  const platformTags = { darwin: 'private_mac_apis', linux: 'gtk4' }[process.platform]
  const tags = platformTags ? ['-tags', platformTags] : []
  run('go', ['build', ...tags, '-o', built, '.'], { cwd: gui, env })
  const helper = withHelper ? path.join(root, 'target/debug/uniclip-quick-panel') : null
  const binary = process.platform === 'darwin' ? makeDevBundle(built, helper) : built

  // A dev build targets the machine it runs on.
  const uiPlatform = { darwin: 'macos', win32: 'windows', linux: 'linux' }[process.platform]
  const childEnv = {
    ...env,
    ...(uiPlatform ? { UC_UI_PLATFORM: uiPlatform } : {}),
    UNICLIPBOARD_ENV: 'development',
    UC_PROFILE: profile,
    UC_DEV_SERVER_PORT: String(port),
    // The host resolves the daemon next to itself or on PATH.
    PATH: `${path.join(root, 'target/debug')}${path.delimiter}${env.PATH ?? ''}`,
  }
  // Run Vite directly so stopping this child also stops the server on Windows.
  const vite = spawn(process.execPath, [path.join(root, 'node_modules/vite/bin/vite.js')], {
    cwd: path.join(gui, 'frontend'),
    env: childEnv,
    stdio: 'inherit',
    windowsHide: true,
  })
  let app
  let stopping = false
  let interrupted = false
  const stop = () => {
    if (stopping) return
    stopping = true
    vite.kill('SIGTERM')
    if (!app?.pid || app.exitCode !== null || app.signalCode !== null) return
    if (process.platform === 'win32') {
      // Node's kill() uses TerminateProcess on Windows, bypassing the host's
      // shutdown hook. Include its detached daemon and helpers in the stop.
      const result = spawnSync('taskkill', ['/PID', String(app.pid), '/T', '/F'], {
        windowsHide: true,
        stdio: 'ignore',
        timeout: 10_000,
      })
      if (result.error)
        console.error(`[wails-dev] failed to stop app process tree: ${result.error.message}`)
    } else {
      app.kill('SIGTERM')
    }
  }
  const interrupt = () => {
    interrupted = true
    stop()
  }
  process.on('SIGINT', interrupt)
  process.on('SIGTERM', interrupt)
  try {
    await waitForServer(port, vite)
    if (stopping) return 130
    app = spawn(binary, [], {
      cwd: root,
      env: { ...childEnv, FRONTEND_DEVSERVER_URL: `http://127.0.0.1:${port}` },
      stdio: 'inherit',
      windowsHide: true,
      // Keep Ctrl+C from terminating the GUI before the launcher stops its tree.
      detached: process.platform === 'win32',
    })
    const code = await new Promise(resolve => {
      app.once('exit', (status, signal) => resolve(status ?? (signal ? 1 : 0)))
      app.once('error', error => {
        console.error(`[wails-dev] failed to start GUI: ${error.message}`)
        resolve(1)
      })
    })
    return interrupted ? 130 : code
  } catch (error) {
    if (!interrupted) console.error(error.message)
    return interrupted ? 130 : 1
  } finally {
    stop()
    process.off('SIGINT', interrupt)
    process.off('SIGTERM', interrupt)
  }
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  process.exitCode = await main(process.argv.slice(2))
}
