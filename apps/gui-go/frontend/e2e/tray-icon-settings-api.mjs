import assert from 'node:assert/strict'
import { spawn, execFileSync } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { existsSync } from 'node:fs'
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { createServer, loadConfigFromFile } from 'vite'

// Nonvisual end-to-end check of the tray icon save path without a browser, GUI or WebView: the real frontend modules
// (getSettings, updateSettings and its patch conversion, the generated SDK and the daemon client) run against an
// isolated real `uniclipd`, and the product settings file is read back from disk. Only the native host is replaced:
// its GetDaemonSession binding is answered through the Wails runtime transport (e2e/fixtures/fake-host.ts) by the
// same token exchange the Go host makes (POST /auth/connect with the daemon.conn bearer token). This proves
// conversion -> API -> storage; it does not show what the native tray displays.
//
//   node e2e/tray-icon-settings-api.mjs <uniclipd> <evidence-dir>
const [binary, evidenceArg] = process.argv.slice(2)
assert.ok(binary && evidenceArg, 'usage: tray-icon-settings-api.mjs <uniclipd> <evidence-dir>')
const evidence = path.resolve(evidenceArg)
await mkdir(evidence, { recursive: true })

const profile = `e2e-tray-ui-${randomBytes(4).toString('hex')}`
const appDir = `app.uniclipboard.desktop-${profile}`
const home = homedir()
const roots =
  process.platform === 'darwin'
    ? {
        data: path.join(home, 'Library/Application Support', appDir),
        cache: path.join(home, 'Library/Caches', appDir),
        logs: path.join(home, 'Library/Logs', appDir),
      }
    : {
        data: path.join(process.env.XDG_DATA_HOME || path.join(home, '.local/share'), appDir),
        cache: path.join(process.env.XDG_CACHE_HOME || path.join(home, '.cache'), appDir),
        logs: path.join(process.env.XDG_STATE_HOME || path.join(home, '.local/state'), appDir),
      }
for (const dir of Object.values(roots)) assert.ok(!existsSync(dir), `${dir} already exists`)
const productFile = path.join(roots.data, 'desktop-settings.json')

const record = {
  source: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
  mode: 'nonvisual-real-frontend-modules-real-daemon',
  profile,
  steps: [],
}
const sha256 = bytes => (bytes ? createHash('sha256').update(bytes).digest('hex') : null)
const readProduct = async () => (existsSync(productFile) ? await readFile(productFile) : null)

const daemonLog = path.join(evidence, 'tray-icon-settings-api-daemon.log')
const logChunks = []
const daemon = spawn(binary, [], {
  env: {
    ...process.env,
    UC_PROFILE: profile,
    UNICLIPBOARD_ENV: 'development',
    UC_DAEMON_RUN_MODE: 'server',
    RUST_LOG: 'warn',
  },
  stdio: ['ignore', 'pipe', 'pipe'],
})
daemon.stdout.on('data', chunk => logChunks.push(chunk))
daemon.stderr.on('data', chunk => logChunks.push(chunk))
const exited = new Promise(resolve => daemon.once('exit', resolve))

let vite
let daemonClient
try {
  const connPath = path.join(roots.data, 'daemon.conn')
  let conn
  for (let i = 0; i < 300 && !conn; i++) {
    try {
      const parsed = JSON.parse(await readFile(connPath, 'utf8'))
      if (parsed.pid === daemon.pid) conn = parsed
    } catch {
      // not written yet
    }
    if (!conn) await new Promise(resolve => setTimeout(resolve, 200))
  }
  assert.ok(conn, 'daemon.conn of the started daemon did not appear')
  const baseUrl = `http://127.0.0.1:${conn.port}`

  // The page's daemon calls carry the request body the frontend built; record each settings write as sent.
  const realFetch = globalThis.fetch
  const sent = []
  globalThis.fetch = async (input, init) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (request.method === 'PUT' && new URL(request.url).pathname === '/settings') {
      sent.push(JSON.parse(await request.clone().text()))
    }
    return realFetch(request)
  }

  // The page modules expect a window: the Wails runtime and the daemon client's wake listeners register on it.
  const events = new EventTarget()
  globalThis.addEventListener = events.addEventListener.bind(events)
  globalThis.removeEventListener = events.removeEventListener.bind(events)
  globalThis.window = globalThis
  // The app's own Vite config (aliases, generated bindings), loaded in Node. Node loads the CommonJS logger itself: the
  // browser build the config aliases to cannot be evaluated by the module runner.
  const loaded = await loadConfigFromFile(
    { command: 'serve', mode: 'test' },
    path.resolve('vite.config.ts')
  )
  vite = await createServer({
    ...loaded.config,
    configFile: false,
    plugins: [],
    resolve: {
      ...loaded.config.resolve,
      alias: loaded.config.resolve.alias.filter(entry => String(entry.find) !== String(/^pino$/)),
    },
    server: { middlewareMode: true, hmr: false },
    appType: 'custom',
    optimizeDeps: { noDiscovery: true, include: [] },
    logLevel: 'warn',
  })
  const { installFakeHost } = await vite.ssrLoadModule('/e2e/fixtures/fake-host.ts')
  installFakeHost({
    // The Go host's GetDaemonSession: exchange the daemon.conn token for a session.
    GetDaemonSession: async () => {
      const response = await realFetch(`${baseUrl}/auth/connect`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${conn.token}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ pid: process.pid, clientType: 'gui' }),
      })
      assert.equal(response.status, 200)
      return (await response.json()).data
    },
  })
  ;({ daemonClient } = await vite.ssrLoadModule('/src/api/daemon/client.ts'))
  const { getSettings, updateSettings } = await vite.ssrLoadModule('/src/api/daemon/settings.ts')
  daemonClient.initialize({ baseUrl, wsUrl: baseUrl.replace('http', 'ws') + '/ws' })

  const initial = await getSettings()
  record.steps.push({
    step: 'initial read',
    trayIcon: initial.general.trayIcon,
    productSha256: sha256(await readProduct()),
  })
  assert.equal(initial.general.trayIcon, 'glyph')

  for (const trayIcon of ['cat', 'glyph']) {
    // The same input the settings page builds (SettingContext.updateGeneralSetting): the current settings with the
    // changed general field merged in.
    const current = await getSettings()
    const result = await updateSettings({ ...current, general: { ...current.general, trayIcon } })
    const body = sent.at(-1)
    const readBack = await getSettings()
    const bytes = await readProduct()
    const stored = bytes ? JSON.parse(bytes.toString('utf8')) : null
    record.steps.push({
      step: `save ${trayIcon}`,
      requestGeneralTrayIcon: body?.general?.trayIcon,
      requestGeneralKeys: Object.keys(body?.general ?? {}).sort(),
      result,
      readBack: readBack.general.trayIcon,
      storedTrayIcon: stored?.general?.trayIcon,
      productSha256: sha256(bytes),
    })
    assert.equal(body?.general?.trayIcon, trayIcon, 'the request body carries trayIcon')
    assert.equal(result.success, true)
    assert.equal(readBack.general.trayIcon, trayIcon)
    assert.equal(stored?.general?.trayIcon, trayIcon, 'desktop-settings.json holds the value')
  }
  record.passed = true
} catch (error) {
  record.passed = false
  record.error = String(error?.stack ?? error)
  process.exitCode = 1
} finally {
  daemonClient?.destroy()
  await vite?.close()
  daemon.kill('SIGTERM')
  await Promise.race([exited, new Promise(resolve => setTimeout(resolve, 15_000))])
  if (daemon.exitCode === null && daemon.signalCode === null) daemon.kill('SIGKILL')
  await writeFile(daemonLog, Buffer.concat(logChunks))
  // Remove only this run's isolated profile directories.
  for (const dir of Object.values(roots)) {
    if (path.basename(dir) === appDir) await rm(dir, { recursive: true, force: true })
  }
  record.profileDirsRemoved = Object.values(roots).every(dir => !existsSync(dir))
  await writeFile(
    path.join(evidence, 'tray-icon-settings-api.json'),
    JSON.stringify(record, null, 2)
  )
  console.log(JSON.stringify(record, null, 2))
}
