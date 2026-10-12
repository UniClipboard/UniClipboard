import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'

// Nonvisual acceptance of the real daemon query contract used by the History input.
// This does not claim to exercise DOM, keyboard, or native WebView behavior.
const out = path.resolve(process.argv[2])
const evidence = path.resolve(process.argv[3])
const daemon = JSON.parse(await readFile(path.join(out, 'daemon.json'), 'utf8'))
assert.ok(daemon.profile.startsWith('gui-go-') && daemon.home.includes('uc-gui-go-'))
const conn = JSON.parse(await readFile(daemon.conn, 'utf8'))
const seed = JSON.parse(await readFile(path.join(out, 'token-seed.json'), 'utf8'))
await mkdir(evidence, { recursive: true })
const result = {
  source: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
  mode: 'nonvisual-real-daemon-api',
  profile: daemon.profile,
  checks: [],
}
async function request(route, authorization, body) {
  const response = await fetch(`${daemon.daemonUrl}${route}`, {
    method: body ? 'POST' : 'GET',
    headers: { Authorization: authorization, 'Content-Type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  })
  return { status: response.status, body: await response.json() }
}
const session = await request('/auth/connect', `Bearer ${conn.token}`, {
  pid: process.pid,
  clientType: 'gui',
})
assert.equal(session.status, 200)
const auth = `Session ${session.body.data.sessionToken}`
assert.equal(
  (await request('/content-lock/unlock', auth, { passphrase: daemon.passphrase })).status,
  200
)
async function query(name, params, expected) {
  const response = await request(
    `/search/query?${new URLSearchParams({ query: '', limit: '100', ...params })}`,
    auth
  )
  result.checks.push({ name, params, ...response })
  assert.equal(response.status, 200, name)
  assert.equal(response.body.data.total, expected, name)
  return response.body.data.items
}
try {
  const all = await query('plain text', { query: 'pix' }, 5)
  assert.ok(all.some(item => item.contentType === 'file'))
  const phone = all.find(item => item.textPreview === 'pix phone synthetic')
  assert.ok(phone?.sourceDevice)
  await query('text type with text query', { query: 'pix', contentTypes: 'text' }, 4)
  await query('file type with text query', { query: 'pix', contentTypes: 'file' }, 1)
  await query('image type without keyword', { contentTypes: 'image' }, 1)
  const source = await query(
    'mobile source with text query',
    { query: 'pix', sourceDevices: phone.sourceDevice },
    1
  )
  assert.equal(source[0].entryId, phone.entryId)
  const tagged = await query('tag with text query', { query: 'pix', tags: seed.tag.tagId }, 1)
  assert.equal(tagged[0].entryId, seed.tagged)
  await query(
    'combined type and tag',
    { query: 'pix', contentTypes: 'text', tags: seed.tag.tagId },
    1
  )
  await query(
    'reversed tag and type',
    { tags: seed.tag.tagId, contentTypes: 'text', query: 'pix' },
    1
  )
  await query(
    'successful empty combination',
    { query: 'pix', contentTypes: 'text', tags: seed.tag.tagId, sourceDevices: phone.sourceDevice },
    0
  )
  // Preserve the raw literal input. The pinned Engine tokenizer currently returns no match
  // for this compound string; this acceptance does not invent substring-search semantics.
  await query(
    'literal input retains current Engine semantics',
    { query: 'pix /tmp/docs user@example.com #raw' },
    0
  )
  const invalid = await request(
    `/search/query?${new URLSearchParams({ query: 'pix AND alpha OR beta' })}`,
    auth
  )
  result.checks.push({ name: 'real syntax rejection', ...invalid })
  assert.equal(invalid.status, 400)
  assert.equal(invalid.body.code, 'invalid_query')
  assert.equal(invalid.body.data, undefined)
  const counts = await request('/search/count', auth, {
    queries: [
      { query: 'pix', contentTypes: 'text' },
      { query: 'pix', tags: seed.tag.tagId },
      { query: 'pix', sourceDevices: phone.sourceDevice },
    ],
  })
  result.checks.push({ name: 'candidate counts retain text', ...counts })
  assert.equal(counts.status, 200)
  assert.deepEqual(counts.body.data.counts, [4, 1, 1])
  result.ok = true
} catch (error) {
  result.ok = false
  result.error = String(error.stack ?? error)
  process.exitCode = 1
} finally {
  await writeFile(path.join(evidence, 'api-result.json'), JSON.stringify(result, null, 2))
}
console.log(
  JSON.stringify(
    {
      ok: result.ok,
      checks: result.checks.map(item => ({ name: item.name, status: item.status })),
      error: result.error,
    },
    null,
    2
  )
)
