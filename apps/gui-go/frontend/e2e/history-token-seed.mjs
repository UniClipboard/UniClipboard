import assert from 'node:assert/strict'
import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'

// Only use the isolated daemon created by history_kit_browser_daemon.py.
const out = path.resolve(process.argv[2])
const daemon = JSON.parse(await readFile(path.join(out, 'daemon.json'), 'utf8'))
assert.ok(daemon.profile.startsWith('gui-go-') && daemon.home.includes('uc-gui-go-'))
const conn = JSON.parse(await readFile(daemon.conn, 'utf8'))
async function post(route, body, auth) {
  const response = await fetch(`${daemon.daemonUrl}${route}`, {
    method: 'POST',
    headers: { Authorization: auth, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  assert.ok(response.ok, `${route}: ${response.status} ${await response.clone().text()}`)
  return (await response.json()).data
}
const session = (
  await post('/auth/connect', { pid: process.pid, clientType: 'gui' }, `Bearer ${conn.token}`)
).sessionToken
const auth = `Session ${session}`
await post('/content-lock/unlock', { passphrase: daemon.passphrase }, auth)
for (const text of [
  'pix alpha synthetic',
  'pix beta synthetic',
  'unrelated synthetic',
  'pix /tmp/docs user@example.com #raw',
]) {
  await post('/clipboard/dispatch', { text }, auth)
}
const entries = await fetch(`${daemon.daemonUrl}/search/query?query=pix&limit=50`, {
  headers: { Authorization: auth },
}).then(r => r.json())
assert.ok(entries.data.items.length >= 3)
const tagged = entries.data.items.find(i => i.textPreview === 'pix alpha synthetic')
assert.ok(tagged)
const created = await post('/history/tags', { name: 'token-work' }, auth)
const tag = created.tag ?? created
assert.ok(tag.tagId, JSON.stringify(created))
await post(`/history/tags/${tag.tagId}/entries/add`, { entryIds: [tagged.entryId] }, auth)
await writeFile(
  path.join(out, 'token-seed.json'),
  JSON.stringify({ entries: entries.data.items, tag, tagged: tagged.entryId }, null, 2)
)
