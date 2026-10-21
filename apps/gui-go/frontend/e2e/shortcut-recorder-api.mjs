import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { existsSync } from 'node:fs'
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { createServer, loadConfigFromFile } from 'vite'

// Nonvisual end-to-end check of the shortcut recorder's output against the daemon's stored token set (ADR-013
// sections 8.1.1 and 10), without a browser, GUI or WebView. The production key mapping (src/shortcuts/record.ts,
// which KeyRecorder uses) turns keydown fields into a binding; every binding it records is written with PUT
// /settings to an isolated real `uniclipd` and read back, and every key it refuses is shown as unsupported. The
// keydown fields are the values browsers report for those keys; no real keyboard event is fired, and nothing here
// shows whether an executor or the OS accepts the binding.
//
//   node e2e/shortcut-recorder-api.mjs <uniclipd> <evidence-dir>
const [binary, evidenceArg] = process.argv.slice(2)
assert.ok(binary && evidenceArg, 'usage: shortcut-recorder-api.mjs <uniclipd> <evidence-dir>')
const evidence = path.resolve(evidenceArg)
await mkdir(evidence, { recursive: true })

const profile = `e2e-shortcut-recorder-${randomBytes(4).toString('hex')}`
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

const status = execFileSync('git', ['status', '--porcelain', '--', '.'], { encoding: 'utf8' })
const record = {
  source: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
  sourceClean: status.trim() === '',
  mode: 'nonvisual-production-recorder-module-real-daemon',
  platform: process.platform,
  profile,
  cases: [],
}

// [name, keydown fields, expected binding or null (refused as unsupported)]
const CASES = [
  ['letter', { key: 'b', code: 'KeyB', ctrlKey: true, altKey: true }, 'ctrl+alt+b'],
  ['digit row 1', { key: '1', code: 'Digit1', ctrlKey: true, altKey: true }, 'ctrl+alt+1'],
  [
    'numpad 1, NumLock on',
    { key: '1', code: 'Numpad1', ctrlKey: true, altKey: true },
    'ctrl+alt+numpad1',
  ],
  [
    'numpad 1, NumLock off',
    { key: 'End', code: 'Numpad1', ctrlKey: true, altKey: true },
    'ctrl+alt+numpad1',
  ],
  [
    'numpad decimal',
    { key: '.', code: 'NumpadDecimal', ctrlKey: true, altKey: true },
    'ctrl+alt+numpaddecimal',
  ],
  ['numpad add', { key: '+', code: 'NumpadAdd', ctrlKey: true, altKey: true }, 'ctrl+alt+add'],
  ['space', { key: ' ', code: 'Space', ctrlKey: true, altKey: true }, 'ctrl+alt+space'],
  [
    'arrow up',
    { key: 'ArrowUp', code: 'ArrowUp', ctrlKey: true, altKey: true },
    'ctrl+alt+arrowup',
  ],
  ['page up', { key: 'PageUp', code: 'PageUp', ctrlKey: true, altKey: true }, 'ctrl+alt+pageup'],
  ['insert', { key: 'Insert', code: 'Insert', ctrlKey: true, altKey: true }, 'ctrl+alt+insert'],
  ['comma', { key: ',', code: 'Comma', ctrlKey: true, altKey: true }, 'ctrl+alt+comma'],
  ['period', { key: '.', code: 'Period', ctrlKey: true, shiftKey: true }, 'ctrl+shift+period'],
  [
    'shifted semicolon (colon)',
    { key: ':', code: 'Semicolon', ctrlKey: true, shiftKey: true },
    'ctrl+shift+semicolon',
  ],
  ['German ä (Quote key)', { key: 'ä', code: 'Quote', ctrlKey: true }, 'ctrl+quote'],
  ['Spanish ñ (Semicolon key)', { key: 'ñ', code: 'Semicolon', ctrlKey: true }, 'ctrl+semicolon'],
  [
    'shifted equal (+)',
    { key: '+', code: 'Equal', ctrlKey: true, shiftKey: true },
    'ctrl+shift+equal',
  ],
  ['minus', { key: '-', code: 'Minus', ctrlKey: true, altKey: true }, 'ctrl+alt+minus'],
  ['slash', { key: '/', code: 'Slash', ctrlKey: true, altKey: true }, 'ctrl+alt+slash'],
  [
    'backslash',
    { key: '\\', code: 'Backslash', ctrlKey: true, altKey: true },
    'ctrl+alt+backslash',
  ],
  [
    'bracket left',
    { key: '[', code: 'BracketLeft', ctrlKey: true, altKey: true },
    'ctrl+alt+bracketleft',
  ],
  ['backquote', { key: '`', code: 'Backquote', ctrlKey: true, altKey: true }, 'ctrl+alt+backquote'],
  ['F13', { key: 'F13', code: 'F13', ctrlKey: true }, 'ctrl+f13'],
  ['F24', { key: 'F24', code: 'F24', ctrlKey: true }, 'ctrl+f24'],
  ['F25', { key: 'F25', code: 'F25', ctrlKey: true }, null],
  ['IntlBackslash (ISO key)', { key: '<', code: 'IntlBackslash', ctrlKey: true }, null],
  ['Unidentified', { key: 'Unidentified', code: '', ctrlKey: true }, null],
  ['macOS Option+Cmd+V (√)', { key: '√', code: 'KeyV', altKey: true, metaKey: true }, 'alt+meta+v'],
  [
    'macOS dead key Option+E',
    { key: 'Dead', code: 'KeyE', altKey: true, metaKey: true },
    'alt+meta+e',
  ],
  [
    'Dvorak v on the Period key',
    { key: 'v', code: 'Period', ctrlKey: true, altKey: true },
    'ctrl+alt+v',
  ],
  [
    'AZERTY a on the KeyQ key',
    { key: 'a', code: 'KeyQ', ctrlKey: true, altKey: true },
    'ctrl+alt+a',
  ],
  [
    'volume up',
    { key: 'AudioVolumeUp', code: 'AudioVolumeUp', ctrlKey: true },
    'ctrl+audiovolumeup',
  ],
]
const event = fields => ({
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...fields,
})

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
const exited = new Promise(resolve =>
  daemon.once('exit', (code, signal) => resolve({ code, signal }))
)

let vite
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
  record.daemonPid = daemon.pid
  const baseUrl = `http://127.0.0.1:${conn.port}`
  const session = await fetch(`${baseUrl}/auth/connect`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${conn.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ pid: process.pid, clientType: 'cli' }),
  })
  assert.equal(session.status, 200)
  const token = (await session.json()).data.sessionToken
  const api = (method, body) =>
    fetch(`${baseUrl}/settings`, {
      method,
      headers: { Authorization: `Session ${token}`, 'Content-Type': 'application/json' },
      body: body && JSON.stringify(body),
    })

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
  const { comboFromEvent } = await vite.ssrLoadModule('/src/shortcuts/record.ts')

  for (const [name, fields, expected] of CASES) {
    const recorded = comboFromEvent(event(fields))
    const entry = { name, keydown: fields, expected, recorded }
    if (expected === null) {
      assert.ok(
        recorded && 'unsupported' in recorded,
        `${name}: expected the key to be refused, got ${JSON.stringify(recorded)}`
      )
    } else {
      assert.deepEqual(recorded, { combo: expected }, name)
      const put = await api('PUT', {
        keyboardShortcuts: { shortcuts: { 'global.toggleQuickPanel': expected } },
      })
      entry.putStatus = put.status
      const read = await (await api('GET')).json()
      entry.readBack = (read.data ?? read).keyboardShortcuts?.['global.toggleQuickPanel']
      if (entry.readBack === undefined) entry.readResponse = read
      assert.equal(put.status, 200, `${name}: the daemon refused ${expected}`)
      assert.equal(entry.readBack, expected, `${name}: read back`)
    }
    record.cases.push(entry)
  }

  // The v1.2 recorder turned Space into a modifier-only binding (the space is the stroke separator). Record what the
  // daemon's token set does with that value; the recorder no longer produces it.
  const legacySpace = await api('PUT', {
    keyboardShortcuts: { shortcuts: { 'global.toggleQuickPanel': 'ctrl+alt' } },
  })
  record.legacySpaceValue = { value: 'ctrl+alt', putStatus: legacySpace.status }
  record.passed = true
} catch (error) {
  record.passed = false
  record.error = String(error?.stack ?? error)
  process.exitCode = 1
} finally {
  await vite?.close()
  daemon.kill('SIGTERM')
  const exit = await Promise.race([
    exited,
    new Promise(resolve => setTimeout(() => resolve(null), 15_000)),
  ])
  if (!exit) daemon.kill('SIGKILL')
  record.daemonExit = exit ?? (await exited)
  await writeFile(path.join(evidence, 'shortcut-recorder-api-daemon.log'), Buffer.concat(logChunks))
  for (const dir of Object.values(roots)) {
    if (path.basename(dir) === appDir) await rm(dir, { recursive: true, force: true })
  }
  record.profileDirsRemoved = Object.values(roots).every(dir => !existsSync(dir))
  await writeFile(
    path.join(evidence, 'shortcut-recorder-api.json'),
    JSON.stringify(record, null, 2)
  )
  console.log(
    JSON.stringify(
      {
        passed: record.passed,
        error: record.error,
        cases: record.cases.length,
        legacySpaceValue: record.legacySpaceValue,
      },
      null,
      2
    )
  )
}
