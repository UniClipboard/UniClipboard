import assert from 'node:assert/strict'
import { spawn, execFileSync } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { existsSync } from 'node:fs'
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { createServer, loadConfigFromFile } from 'vite'

// Nonvisual end-to-end check of the window's settings authority and save paths (ADR-013 section 9) without a
// browser, GUI or WebView. The production modules run in Node against an isolated real `uniclipd`:
// `@/lib/settings-sync` (snapshot-gated reads, the read/write queue, read after every write), `saveSettingsPatch`
// (the body SettingContext sends for each control), `updateSettings` with its partial-failure error, the generated
// SDK, the daemon client and the daemon WebSocket client over Node's WebSocket. Only the native host is replaced:
// GetDaemonConnectionInfo and GetDaemonSession are answered through the Wails runtime transport
// (e2e/fixtures/fake-host.ts) by the same token exchange the Go host makes. Writes labeled "go-host body" or
// "other client" are raw HTTP requests with the exact bodies of the cited Go sources or of another window; they are
// not Go host runs. Interposers only record or delay traffic; no response is invented.
//
//   node e2e/settings-sync-api.mjs <uniclipd> <evidence-dir>
const [binary, evidenceArg] = process.argv.slice(2)
assert.ok(binary && evidenceArg, 'usage: settings-sync-api.mjs <uniclipd> <evidence-dir>')
const evidence = path.resolve(evidenceArg)
await mkdir(evidence, { recursive: true })

const profile = `e2e-settings-sync-${randomBytes(4).toString('hex')}`
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
const legacyFile = path.join(roots.data, 'settings.json')
const telemetryFile = path.join(roots.data, 'desktop-telemetry.json')
const saveDir = path.join(evidence, `${profile}-autosave`)

const record = {
  source: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
  sourceClean: execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim() === '',
  binarySha256: createHash('sha256')
    .update(await readFile(binary))
    .digest('hex'),
  mode: 'nonvisual-real-frontend-modules-real-daemon',
  profile,
  cases: [],
}
const sha256 = bytes => (bytes ? createHash('sha256').update(bytes).digest('hex') : null)
const readJson = async file => (existsSync(file) ? JSON.parse(await readFile(file, 'utf8')) : null)
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))
async function until(what, check, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const value = await check()
    if (value) return value
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`)
    await sleep(50)
  }
}

// ---- wire log: every daemon HTTP request and WS frame the page modules exchange
const wire = []
let seq = 0
const note = entry => {
  const item = { seq: ++seq, at: Date.now(), ...entry }
  wire.push(item)
  return item
}
const realFetch = globalThis.fetch
let holdNextSettingsGet = null // { reached: resolve, release: Promise }
globalThis.fetch = async (input, init) => {
  const request = input instanceof Request ? input : new Request(input, init)
  const { pathname } = new URL(request.url)
  const body = request.method === 'PUT' ? JSON.parse(await request.clone().text()) : undefined
  const sent = note({ kind: 'http', method: request.method, path: pathname, body, origin: 'page' })
  const response = await realFetch(request)
  const text = await response.clone().text()
  let payload
  try {
    payload = JSON.parse(text)
  } catch {
    payload = text
  }
  if (request.method === 'GET' && pathname === '/settings' && holdNextSettingsGet) {
    const hold = holdNextSettingsGet
    holdNextSettingsGet = null
    hold.reached(payload)
    await hold.release
  }
  sent.status = response.status
  sent.response =
    pathname === '/settings' && request.method === 'GET' ? summarize(payload?.data) : payload
  sent.doneAt = Date.now()
  sent.doneSeq = ++seq
  return response
}

// Node's WebSocket under the daemon WS client: frames are recorded; settings frames can be held in order.
const RealWebSocket = globalThis.WebSocket
const sockets = []
let holdSettingsFrames = null // array while holding
class RecordingWebSocket extends RealWebSocket {
  constructor(...args) {
    super(...args)
    sockets.push(this)
  }
  set onmessage(handler) {
    super.onmessage = event => {
      let frame
      try {
        frame = JSON.parse(event.data)
      } catch {
        frame = {}
      }
      const type = frame.type ?? frame.event_type
      if (type && type !== 'heartbeat' && frame.topic !== 'heartbeat') {
        note({ kind: 'ws', type, payload: frame.payload, held: Boolean(holdSettingsFrames) })
      }
      if (holdSettingsFrames && frame.topic === 'settings')
        holdSettingsFrames.push(() => handler(event))
      else handler(event)
    }
  }
  get onmessage() {
    return super.onmessage
  }
}
globalThis.WebSocket = RecordingWebSocket

function summarize(settings) {
  if (!settings) return settings
  return {
    trayIcon: settings.general?.trayIcon,
    theme: settings.general?.theme,
    themeColorLight: settings.general?.themeColorLight,
    language: settings.general?.language,
    deviceName: settings.general?.deviceName,
    autoDownloadUpdate: settings.general?.autoDownloadUpdate,
    debugMode: settings.general?.debugMode,
    telemetryEnabled: settings.general?.telemetryEnabled,
    syncEnabled: settings.sync?.syncEnabled,
    retentionEnabled: settings.retentionPolicy?.enabled,
    autoSaveDir: settings.fileSync?.autoSaveDir,
    allowOverlayNetworkAddrs: settings.network?.allowOverlayNetworkAddrs,
    keyboardShortcuts: settings.keyboardShortcuts,
  }
}

// ---- the daemon
let daemon
let daemonExited
let conn
const logChunks = []
async function startDaemon() {
  daemon = spawn(binary, [], {
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
  daemonExited = new Promise(resolve => daemon.once('exit', resolve))
  const connPath = path.join(roots.data, 'daemon.conn')
  conn = await until(
    'daemon.conn of the started daemon',
    async () => {
      try {
        const parsed = JSON.parse(await readFile(connPath, 'utf8'))
        return parsed.pid === daemon.pid ? parsed : null
      } catch {
        return null
      }
    },
    60_000
  )
  await until('daemon health', async () => {
    try {
      return (await realFetch(`${baseUrl()}/health`)).ok
    } catch {
      return false
    }
  })
}
async function stopDaemon() {
  daemon.kill('SIGTERM')
  await Promise.race([daemonExited, sleep(15_000)])
  if (daemon.exitCode === null && daemon.signalCode === null) daemon.kill('SIGKILL')
  await daemonExited
}
const baseUrl = () => `http://127.0.0.1:${conn.port}`

// Another client (a second window, the Go host or the CLI): its own session, raw HTTP.
async function otherSession() {
  const response = await realFetch(`${baseUrl()}/auth/connect`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${conn.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ pid: process.pid, clientType: 'cli' }),
  })
  assert.equal(response.status, 200)
  return (await response.json()).data.sessionToken
}
async function otherPut(label, body) {
  const token = await otherSession()
  const sent = note({ kind: 'http', method: 'PUT', path: '/settings', body, origin: label })
  const response = await realFetch(`${baseUrl()}/settings`, {
    method: 'PUT',
    headers: { Authorization: `Session ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  sent.status = response.status
  sent.response = await response.json()
  sent.doneSeq = ++seq
  return sent
}
async function otherGet() {
  const token = await otherSession()
  const response = await realFetch(`${baseUrl()}/settings`, {
    headers: { Authorization: `Session ${token}` },
  })
  assert.equal(response.status, 200)
  return (await response.json()).data
}

let vite
let daemonClient
const startSeq = () => seq
const pagePuts = from =>
  wire.filter(e => e.seq > from && e.kind === 'http' && e.method === 'PUT' && e.origin === 'page')
const pageGets = from =>
  wire.filter(
    e => e.seq > from && e.kind === 'http' && e.method === 'GET' && e.path === '/settings'
  )
async function product() {
  const bytes = existsSync(productFile) ? await readFile(productFile) : null
  return { sha256: sha256(bytes), json: bytes ? JSON.parse(bytes.toString('utf8')) : null }
}

try {
  // Raw invalid shortcut kept by the task 4 import: the legacy file has a recorder value outside the token set.
  await mkdir(roots.data, { recursive: true })
  await writeFile(
    legacyFile,
    JSON.stringify({
      schema_version: 3,
      keyboard_shortcuts: { 'global.toggleQuickPanel': 'page up' },
    })
  )
  await mkdir(saveDir, { recursive: true })
  await startDaemon()

  const events = new EventTarget()
  globalThis.addEventListener = events.addEventListener.bind(events)
  globalThis.removeEventListener = events.removeEventListener.bind(events)
  globalThis.window = globalThis
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
    // The Go host's answers: where the daemon is, and a session from the daemon.conn token.
    GetDaemonConnectionInfo: async () => ({
      baseUrl: baseUrl(),
      wsUrl: `ws://127.0.0.1:${conn.port}/ws`,
    }),
    GetDaemonSession: async () => {
      const response = await realFetch(`${baseUrl()}/auth/connect`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${conn.token}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ pid: process.pid, clientType: 'gui' }),
      })
      assert.equal(response.status, 200)
      return (await response.json()).data
    },
  })
  ;({ daemonClient } = await vite.ssrLoadModule('/src/api/daemon/client.ts'))
  const sync = await vite.ssrLoadModule('/src/lib/settings-sync.ts')
  const { updateSettings, SettingsPartiallyAppliedError, DaemonApiError } =
    await vite.ssrLoadModule('/src/api/daemon/index.ts')
  const { reconnectDaemonWs } = await vite.ssrLoadModule('/src/lib/daemon-ws-bootstrap.ts')
  const { buildKeyboardShortcutsPatch } = await vite.ssrLoadModule(
    '/src/api/tauri-command/settings.ts'
  )

  const published = []
  const state = () => sync.getSettingsSyncState().settings
  const waitState = (what, check) => until(what, () => state() && check(state()))

  // ---- C1 (F69/F70 gate): the first read waits for settings.snapshot; a write made after the subscription and
  // before the read is not missed.
  {
    const from = startSeq()
    holdSettingsFrames = []
    sync.subscribeSettings(({ settings }) => published.push(summarize(settings)))
    await until('a held settings.snapshot', () =>
      wire.some(e => e.seq > from && e.kind === 'ws' && e.type === 'settings.snapshot' && e.held)
    )
    const getsBeforeSnapshot = pageGets(from).length
    const write = await otherPut('other client', { general: { trayIcon: 'cat' } })
    await sleep(300)
    const getsWhileHeld = pageGets(from).length
    const held = holdSettingsFrames
    holdSettingsFrames = null
    for (const deliver of held) deliver()
    await waitState('the first read', s => s.general.trayIcon === 'cat')
    const firstGet = pageGets(from)[0]
    const snapshot = wire.find(
      e => e.seq > from && e.kind === 'ws' && e.type === 'settings.snapshot'
    )
    record.cases.push({
      case: 'C1 snapshot gates the first read',
      getsBeforeSnapshot,
      getsWhileHeld,
      writeBetweenSubscribeAndRead: { body: write.body, status: write.status },
      snapshotSeq: snapshot.seq,
      firstGetSeq: firstGet.seq,
      firstGetResponse: firstGet.response,
    })
    assert.equal(getsBeforeSnapshot, 0, 'no read before the snapshot')
    assert.equal(getsWhileHeld, 0, 'no read while the snapshot is held')
    assert.equal(write.status, 200)
    assert.equal(firstGet.response.trayIcon, 'cat')
  }

  // ---- C2 (F64/F65): each control sends its own leaf; only debugMode and network ask for a restart.
  {
    const steps = [
      ['tray icon (TrayIconSettings)', { general: { trayIcon: 'glyph' } }, false],
      ['theme (AppearanceSection)', { general: { theme: 'dark' } }, false],
      [
        'light preset (AppearanceSection)',
        { general: { themeColorLight: 'rose', themeColor: null } },
        false,
      ],
      ['debug mode on (PUT path)', { general: { debugMode: true } }, true],
      ['debug mode off (PUT path)', { general: { debugMode: false } }, true],
      ['network (NetworkSection)', { network: { allowOverlayNetworkAddrs: true } }, true],
    ]
    for (const [label, patch, restart] of steps) {
      const from = startSeq()
      const result = await sync.saveSettingsPatch(patch)
      const [put] = pagePuts(from)
      const readAfter = pageGets(from).find(e => e.seq > put.seq)
      record.cases.push({
        case: `C2 ${label}`,
        body: put.body,
        result,
        readAfterPut: Boolean(readAfter),
      })
      assert.deepEqual(put.body, patch, `${label}: the body holds exactly the changed leaves`)
      assert.equal(result.restartRequired, restart, `${label}: restartRequired`)
      assert.ok(readAfter, `${label}: the window reads after the save`)
    }
    const s = state()
    assert.equal(s.general.trayIcon, 'glyph')
    assert.equal(s.general.theme, 'dark')
    assert.equal(s.general.themeColorLight, 'rose')
    assert.equal(s.general.debugMode, false)
    assert.equal(s.network.allowOverlayNetworkAddrs, true)
    // Control: the pre-task-5 SettingContext input (current settings with one general field changed).
    const from = startSeq()
    const legacy = await updateSettings({ ...s, general: { ...s.general, trayIcon: 'glyph' } })
    const [put] = pagePuts(from)
    record.cases.push({
      case: 'C2 control: whole-section body as before task 5',
      generalKeys: Object.keys(put.body.general).sort(),
      hasNetwork: 'network' in put.body,
      result: legacy,
    })
    assert.equal(legacy.restartRequired, true, 'the control reproduces the old false restart')
  }

  // ---- C3 (F67): false and "" are values.
  {
    const results = []
    for (const patch of [
      { sync: { syncEnabled: false } },
      { fileSync: { autoSaveDir: saveDir } },
      { fileSync: { autoSaveDir: '' } },
    ]) {
      const from = startSeq()
      await sync.saveSettingsPatch(patch)
      results.push({ body: pagePuts(from)[0].body, after: summarize(state()) })
    }
    record.cases.push({ case: 'C3 explicit false and empty string', results })
    assert.equal(results[0].body.sync.syncEnabled, false)
    assert.equal(results[0].after.syncEnabled, false)
    assert.equal(results[1].after.autoSaveDir, saveDir)
    assert.equal(results[2].body.fileSync.autoSaveDir, '')
    assert.notEqual(results[2].after.autoSaveDir, saveDir, '"" clears the directory')
  }

  // ---- C4 (F66, B7): concurrent saves from this window, another window and the Go host bodies.
  {
    const before = state()
    const from = startSeq()
    await Promise.all([
      sync.saveSettingsPatch({ general: { theme: 'light' } }),
      sync.saveSettingsPatch({ general: { trayIcon: 'cat' } }),
      sync.saveSettingsPatch({ retentionPolicy: { enabled: !before.retentionPolicy.enabled } }),
      otherPut('go-host body apps/gui-go/tray.go toggleSync', { sync: { syncEnabled: true } }),
      otherPut('go-host body apps/gui-go/host_updater.go setAutoDownload', {
        general: { autoDownloadUpdate: !before.general.autoDownloadUpdate },
      }),
      otherPut('other window LanguageSettings', { general: { language: 'ja-JP' } }),
    ])
    await sync.refreshSettings()
    const after = await otherGet()
    record.cases.push({
      case: 'C4 concurrent independent saves (B7)',
      bodies: wire
        .filter(e => e.seq > from && e.method === 'PUT')
        .map(e => ({ origin: e.origin, body: e.body, status: e.status })),
      after: summarize(after),
      windowCopy: summarize(state()),
    })
    assert.equal(after.general.theme, 'light')
    assert.equal(after.general.trayIcon, 'cat')
    assert.equal(after.retentionPolicy.enabled, !before.retentionPolicy.enabled)
    assert.equal(after.sync.syncEnabled, true)
    assert.equal(after.general.autoDownloadUpdate, !before.general.autoDownloadUpdate)
    assert.equal(after.general.language, 'ja-JP')
    assert.deepEqual(
      summarize(state()),
      summarize(after),
      'the window copy equals the stored state'
    )
  }

  // ---- C5 (F68, F70): a read issued before a write cannot replace the newer read.
  {
    const from = startSeq()
    const publishedFrom = published.length
    let reached
    const reachedPromise = new Promise(resolve => {
      reached = resolve
    })
    let release
    holdNextSettingsGet = { reached, release: new Promise(resolve => (release = resolve)) }
    void sync.refreshSettings()
    const stale = await reachedPromise
    assert.equal(stale.data.general.trayIcon, 'cat')
    const write = await otherPut('other client', { general: { trayIcon: 'glyph' } })
    await until('settings.changed for the write', () =>
      wire.some(e => e.seq > write.seq && e.kind === 'ws' && e.type === 'settings.changed')
    )
    await sleep(300)
    const getsWhileHeld = pageGets(from).length
    release()
    await waitState('the newer read', s => s.general.trayIcon === 'glyph')
    await sleep(300)
    const gets = pageGets(from)
    const sequence = published.slice(publishedFrom).map(s => s.trayIcon)
    record.cases.push({
      case: 'C5 stale read ordering and external change',
      getsWhileHeld,
      gets: gets.map(e => ({ seq: e.seq, doneSeq: e.doneSeq, trayIcon: e.response?.trayIcon })),
      publishedTrayIcon: sequence,
    })
    assert.equal(getsWhileHeld, 1, 'the next read waits for the held one')
    assert.ok(gets[1].seq > gets[0].doneSeq, 'reads do not overlap')
    assert.equal(sequence.at(-1), 'glyph')
    assert.ok(
      !sequence.slice(sequence.indexOf('glyph')).includes('cat'),
      'no older value after the newer one'
    )
  }

  // ---- C6 (F71): a change made while the socket is down arrives through the reconnect snapshot.
  {
    const from = startSeq()
    const socket = sockets.at(-1)
    socket.close()
    await until('socket closed', () => socket.readyState === RealWebSocket.CLOSED)
    const write = await otherPut('other client', { general: { theme: 'dark' } })
    await waitState('the reconnect read', s => s.general.theme === 'dark')
    record.cases.push({
      case: 'C6 reconnect after a missed event',
      writeSeq: write.seq,
      framesAfter: wire.filter(e => e.seq > from && e.kind === 'ws').map(e => e.type),
      getsAfter: pageGets(from).length,
    })
    assert.ok(
      wire.some(e => e.seq > write.seq && e.kind === 'ws' && e.type === 'settings.snapshot'),
      'a snapshot after the reconnect'
    )
  }

  // ---- C11 (F70, removal of settings://sync-changed): an Engine-owned write from another client reaches the window
  // through the Engine settings.changed alone; nothing in the page asks for a read.
  {
    const target = !state().sync.syncEnabled
    const from = startSeq()
    const write = await otherPut('go-host body apps/gui-go/tray.go toggleSync', {
      sync: { syncEnabled: target },
    })
    await waitState('the Engine change in the window', s => s.sync.syncEnabled === target)
    const changed = wire.find(
      e => e.seq > write.seq && e.kind === 'ws' && e.type === 'settings.changed'
    )
    const read = pageGets(from).find(e => e.seq > changed.seq)
    record.cases.push({
      case: 'C11 Engine-owned external write, no explicit refresh',
      body: write.body,
      status: write.status,
      event: changed.payload,
      readAfterEventSeq: read?.seq,
      windowSyncEnabled: state().sync.syncEnabled,
    })
    assert.equal(changed.payload.owner, 'engine')
    assert.ok(changed.payload.sections.includes('sync'))
    assert.ok(read, 'the event caused the read')
  }

  // ---- C7 (F72/F76): a rejected save reports the error after a read.
  {
    const before = summarize(state())
    const from = startSeq()
    let error
    await sync.saveSettingsPatch({ general: { theme: 'neon' } }).catch(err => (error = err))
    const [put] = pagePuts(from)
    const readAfter = pageGets(from).find(e => e.seq > put.doneSeq)
    record.cases.push({
      case: 'C7 400 then read',
      body: put.body,
      status: put.status,
      error: String(error),
      readAfterError: Boolean(readAfter),
      after: summarize(state()),
    })
    assert.ok(error instanceof DaemonApiError)
    assert.equal(put.status, 400)
    assert.ok(readAfter)
    assert.deepEqual(summarize(state()), before)
  }

  // ---- C8 (F73, B8): Engine written, product write failed; then product written, telemetry failed.
  {
    const results = []
    for (const [label, file, patch] of [
      [
        'product file replaced by a directory',
        productFile,
        { general: { deviceName: 'e2e-b8-device', trayIcon: 'cat' } },
      ],
      [
        'telemetry file replaced by a directory',
        telemetryFile,
        { general: { trayIcon: 'cat', telemetryEnabled: false } },
      ],
      [
        'Engine and product written, telemetry file replaced by a directory',
        telemetryFile,
        { general: { deviceName: 'e2e-b8-device-2', trayIcon: 'glyph', telemetryEnabled: false } },
      ],
    ]) {
      const saved = existsSync(file) ? await readFile(file) : null
      await rm(file, { force: true })
      await mkdir(file)
      const from = startSeq()
      let error
      await sync.saveSettingsPatch(patch).catch(err => (error = err))
      const [put] = pagePuts(from)
      const readAfter = pageGets(from).find(e => e.seq > put.doneSeq)
      results.push({
        label,
        body: put.body,
        status: put.status,
        response: put.response,
        errorName: error?.name,
        applied: error?.applied,
        failed: error?.failed,
        skipped: error?.skipped,
        readAfterError: Boolean(readAfter),
        windowCopy: summarize(state()),
      })
      await rm(file, { recursive: true, force: true })
      if (saved) await writeFile(file, saved)
      assert.ok(error instanceof SettingsPartiallyAppliedError, `${label}: typed error`)
      assert.equal(put.status, 500)
      assert.ok(readAfter, `${label}: read after the error`)
    }
    record.cases.push({ case: 'C8 partial failure (B8)', results })
    assert.deepEqual([results[0].applied, results[0].failed], [['engine'], ['desktop']])
    assert.equal(
      results[0].windowCopy.deviceName,
      'e2e-b8-device',
      'the Engine part stays written and shown'
    )
    assert.equal(results[0].windowCopy.trayIcon, 'glyph', 'the product part was not written')
    assert.deepEqual([results[1].applied, results[1].failed], [['desktop'], ['telemetry']])
    assert.equal(results[1].windowCopy.trayIcon, 'cat')
    assert.deepEqual(
      [results[2].applied, results[2].failed],
      [['engine', 'desktop'], ['telemetry']]
    )
    assert.equal(results[2].windowCopy.deviceName, 'e2e-b8-device-2', 'the Engine part is shown')
    assert.equal(results[2].windowCopy.trayIcon, 'glyph', 'the product part is shown')
    assert.equal(results[2].windowCopy.telemetryEnabled, true, 'the consent was not written')
  }

  // ---- C9 (F75/F76): a shortcut save sends the changed id only; the raw invalid value stays; a recorder value
  // outside the token set is rejected and the window reads again. The Go command (merge, OS preflight) is not run:
  // the body is the `patch` it forwards (apps/gui-go/host_commands_quick_panel.go updateKeyboardShortcuts).
  {
    const productBefore = await product()
    const current = state().keyboardShortcuts ?? {}
    const okPatch = buildKeyboardShortcutsPatch(current, {
      ...current,
      'quickPanel.windowIncrease': 'ctrl+k',
    })
    const ok = await sync.writeSettings(() =>
      otherPut('go-host body updateKeyboardShortcuts', {
        keyboardShortcuts: { shortcuts: okPatch },
      })
    )
    const productAfter = await product()
    const from = startSeq()
    const badPatch = buildKeyboardShortcutsPatch(state().keyboardShortcuts, {
      ...state().keyboardShortcuts,
      'quickPanel.windowIncrease': 'ctrl+f25',
    })
    let error
    await sync
      .writeSettings(async () => {
        const sent = await otherPut('go-host body updateKeyboardShortcuts', {
          keyboardShortcuts: { shortcuts: badPatch },
        })
        if (sent.status !== 200) throw new Error(`rejected ${sent.status}`)
      })
      .catch(err => (error = err))
    const readAfter = pageGets(from).length > 0
    record.cases.push({
      case: 'C9 shortcuts',
      getShowsRawInvalid: 'global.toggleQuickPanel' in current,
      okPatch,
      okStatus: ok.status,
      productRawBefore: productBefore.json?.keyboardShortcuts,
      productRawAfter: productAfter.json?.keyboardShortcuts,
      badPatch,
      badError: String(error),
      readAfterError: readAfter,
      windowShortcuts: state().keyboardShortcuts,
    })
    assert.deepEqual(okPatch, { 'quickPanel.windowIncrease': 'ctrl+k' })
    assert.equal(ok.status, 200)
    assert.equal(
      productAfter.json.keyboardShortcuts['global.toggleQuickPanel'],
      'page up',
      'raw value kept'
    )
    assert.deepEqual(badPatch, { 'quickPanel.windowIncrease': 'ctrl+f25' })
    assert.match(String(error), /rejected 400/)
    assert.ok(readAfter)
    assert.equal(state().keyboardShortcuts['quickPanel.windowIncrease'], 'ctrl+k')
  }

  // ---- C10 (F69): a restarted daemon starts its revisions at 0; its snapshot still causes a read.
  {
    const lastRevision = wire
      .filter(e => e.kind === 'ws' && e.type === 'settings.changed')
      .at(-1)?.payload
    const offlineValue = state().general.trayIcon === 'cat' ? 'glyph' : 'cat'
    await stopDaemon()
    const file = await readJson(productFile)
    file.general.trayIcon = offlineValue
    await writeFile(productFile, JSON.stringify(file, null, 2))
    const offlineSha = sha256(await readFile(productFile))
    await startDaemon()
    const from = startSeq()
    // What the page does on the host's app://daemon-connection-changed event.
    await reconnectDaemonWs()
    await waitState('the read after the restart', s => s.general.trayIcon === offlineValue)
    const snapshot = wire.find(
      e => e.seq > from && e.kind === 'ws' && e.type === 'settings.snapshot'
    )
    record.cases.push({
      case: 'C10 restart with an offline edit',
      lastChangedBeforeRestart: lastRevision,
      snapshotAfterRestart: snapshot?.payload,
      offlineProductSha256: offlineSha,
      windowTrayIcon: state().general.trayIcon,
    })
    assert.equal(lastRevision.owner, 'desktop')
    assert.ok(
      snapshot.payload.revisions.desktop < lastRevision.revision,
      'the new run starts lower'
    )
  }

  record.passed = true
} catch (error) {
  record.passed = false
  record.error = String(error?.stack ?? error)
  process.exitCode = 1
} finally {
  daemonClient?.destroy()
  await vite?.close()
  if (daemon) await stopDaemon().catch(() => {})
  await writeFile(path.join(evidence, 'settings-sync-api-daemon.log'), Buffer.concat(logChunks))
  record.finalProduct = await product().catch(() => null)
  for (const dir of Object.values(roots)) {
    if (path.basename(dir) === appDir) await rm(dir, { recursive: true, force: true })
  }
  await rm(saveDir, { recursive: true, force: true })
  record.profileDirsRemoved = Object.values(roots).every(dir => !existsSync(dir))
  record.wire = wire
  await writeFile(path.join(evidence, 'settings-sync-api.json'), JSON.stringify(record, null, 2))
  console.log(
    JSON.stringify(
      { passed: record.passed, error: record.error, cases: record.cases.map(c => c.case) },
      null,
      2
    )
  )
  process.exit()
}
