import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'

// Seeds an isolated daemon for history-kit-browser.mjs over HTTP with one GUI session (the daemon rate-limits
// /auth/connect, so one CLI call per entry is not an option). Used by apps/gui-go/e2e/windows_history_kit_remote.py:
//   node e2e/history-kit-seed.mjs <out-dir> <daemon-url> <daemon.conn> <passphrase> paging <count>
//   node e2e/history-kit-seed.mjs <out-dir> <daemon-url> <daemon.conn> <passphrase> texts <profile> <home> <seeded>
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

if (phase === 'paging') {
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
