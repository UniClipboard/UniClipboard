import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { gradientPng, HISTORY_TEXTS } from './history-seed-data.mjs'

// Seeds an isolated daemon for history-kit-browser.mjs over HTTP with one GUI session (the daemon rate-limits
// /auth/connect, so one CLI call per entry is not an option). Used by apps/gui-go/e2e/windows_history_kit_remote.py:
//   node e2e/history-kit-seed.mjs <out-dir> <daemon-url> <daemon.conn> <passphrase> paging <count>
//   node e2e/history-kit-seed.mjs <out-dir> <daemon-url> <daemon.conn> <passphrase> image <mobile-url> <user> <password>
//   node e2e/history-kit-seed.mjs <out-dir> <daemon-url> <daemon.conn> <passphrase> texts <profile> <home> <seeded>
// `image` makes a real image entry without a paired peer: the daemon's SyncClipboard-compatible mobile LAN listener
// (`uniclip mobile setup --ip 127.0.0.1`) takes `PUT /file/<name>` and then `PUT /SyncClipboard.json` (type Image).
// `texts` sends the entries the browser steps look for (newest last) and writes <out-dir>/daemon.json.
const [out, base, connPath, passphrase, phase, ...rest] = process.argv.slice(2)
const TEXTS = HISTORY_TEXTS

async function post(pathname, body, authorization) {
  const response = await fetch(`${base}${pathname}`, {
    method: 'POST',
    headers: { Authorization: authorization, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!response.ok) throw new Error(`${pathname}: ${response.status} ${await response.text()}`)
  return response.json()
}

const conn = JSON.parse(await readFile(connPath, 'utf8'))
const session = (
  await post('/auth/connect', { pid: process.pid, clientType: 'gui' }, `Bearer ${conn.token}`)
).data.sessionToken
await post('/content-lock/unlock', { passphrase }, `Session ${session}`)
const send = text => post('/clipboard/dispatch', { text }, `Session ${session}`)

if (phase === 'image') {
  const [mobileUrl, user, password] = rest
  const auth = `Basic ${Buffer.from(`${user}:${password}`).toString('base64')}`
  const name = 'gradient-sample.png'
  const put = async (pathname, body, type) => {
    const response = await fetch(`${mobileUrl}${pathname}`, {
      method: 'PUT',
      headers: { Authorization: auth, 'Content-Type': type },
      body,
    })
    if (!response.ok) throw new Error(`${pathname}: ${response.status} ${await response.text()}`)
  }
  await put(`/file/${name}`, gradientPng(), 'image/png')
  await put(
    '/SyncClipboard.json',
    JSON.stringify({ type: 'Image', text: name, dataName: name, hasData: true }),
    'application/json'
  )
} else if (phase === 'paging') {
  for (let index = 0; index < Number(rest[0]); index++) {
    await send(`Paging entry ${String(index).padStart(3, '0')}`)
  }
} else if (phase === 'texts') {
  const [profile, home, seeded] = rest
  // The steps read English labels; the isolated profile's UI language is fixed to en-US (a test VM's system
  // locale may be anything).
  const settings = await fetch(`${base}/settings`, {
    method: 'PUT',
    headers: { Authorization: `Session ${session}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ general: { language: 'en-US' } }),
  })
  if (!settings.ok) throw new Error(`/settings: ${settings.status} ${await settings.text()}`)
  if (Number(seeded) > 0) {
    for (const text of TEXTS) {
      await send(text)
      await new Promise(resolve => setTimeout(resolve, 200))
    }
  }
  await writeFile(path.join(out, 'daemon.conn'), JSON.stringify({ token: conn.token }))
  await writeFile(
    path.join(out, 'daemon.json'),
    `${JSON.stringify(
      {
        home,
        profile,
        daemonUrl: base,
        conn: path.join(out, 'daemon.conn'),
        passphrase,
        seeded: Number(seeded),
      },
      null,
      2
    )}\n`
  )
} else {
  throw new Error(`unknown phase ${phase}`)
}
