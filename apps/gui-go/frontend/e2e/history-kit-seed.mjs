import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { deflateSync } from 'node:zlib'

// Seeds an isolated daemon for history-kit-browser.mjs over HTTP with one GUI session (the daemon rate-limits
// /auth/connect, so one CLI call per entry is not an option). Used by apps/gui-go/e2e/windows_history_kit_remote.py:
//   node e2e/history-kit-seed.mjs <out-dir> <daemon-url> <daemon.conn> <passphrase> paging <count>
//   node e2e/history-kit-seed.mjs <out-dir> <daemon-url> <daemon.conn> <passphrase> image <mobile-url> <user> <password>
//   node e2e/history-kit-seed.mjs <out-dir> <daemon-url> <daemon.conn> <passphrase> texts <profile> <home> <seeded>
// `image` makes a real image entry without a paired peer: the daemon's SyncClipboard-compatible mobile LAN listener
// (`uniclip mobile setup --ip 127.0.0.1`) takes `PUT /file/<name>` and then `PUT /SyncClipboard.json` (type Image).
// `texts` sends the entries the browser steps look for (newest last) and writes <out-dir>/daemon.json.
const [out, base, connPath, passphrase, phase, ...rest] = process.argv.slice(2)
const TEXTS = [
  'Q4 roadmap: ship LAN-only mode, then per-device delivery receipts',
  'https://github.com/UniClipboard/UniClipboard',
  'git rebase -i HEAD~3',
  'docker compose -f compose.prod.yml \\\n  --env-file .env.prod \\\n  up -d --build --remove-orphans',
  'Meeting notes\n- decide the Windows title bar layout\n- review keyboard focus order',
]

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

// A 160 x 100 RGB gradient PNG, so the thumbnail and the detail show a recognisable picture.
function gradientPng(width = 160, height = 100) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    return c >>> 0
  })
  const crc = bytes => {
    let c = 0xffffffff
    for (const b of bytes) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8)
    return (c ^ 0xffffffff) >>> 0
  }
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
    const out = Buffer.alloc(body.length + 8)
    out.writeUInt32BE(data.length, 0)
    body.copy(out, 4)
    out.writeUInt32BE(crc(body), body.length + 4)
    return out
  }
  const header = Buffer.alloc(13)
  header.writeUInt32BE(width, 0)
  header.writeUInt32BE(height, 4)
  header[8] = 8 // bit depth
  header[9] = 2 // RGB
  const raw = Buffer.alloc((width * 3 + 1) * height)
  for (let y = 0; y < height; y++) {
    const row = y * (width * 3 + 1)
    for (let x = 0; x < width; x++) {
      raw[row + 1 + x * 3] = 40 + Math.round((x / width) * 180)
      raw[row + 2 + x * 3] = 90 + Math.round((y / height) * 120)
      raw[row + 3 + x * 3] = 200 - Math.round((x / width) * 120)
    }
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ])
}

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
