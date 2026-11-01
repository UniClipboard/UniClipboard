import assert from 'node:assert/strict'

// Authenticate the owned browser driver without revoking an already-active global content grant.
export async function openGuiSession({ daemonUrl, fileToken, passphrase }) {
  const response = await fetch(`${daemonUrl}/auth/connect`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${fileToken}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ pid: process.pid, clientType: 'gui' }),
  })
  assert.ok(response.ok, `auth/connect: ${response.status}`)
  const token = (await response.json()).data.sessionToken
  const access = await fetch(`${daemonUrl}/content-lock`, {
    headers: { Authorization: `Session ${token}` },
  })
  assert.ok(access.ok, 'read authoritative content grant')
  if (!(await access.json()).data.unlocked) {
    const unlock = await fetch(`${daemonUrl}/content-lock/unlock`, {
      method: 'POST',
      headers: { Authorization: `Session ${token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ passphrase }),
    })
    assert.ok(unlock.ok, `content-lock/unlock: ${unlock.status} ${await unlock.text()}`)
  }
  return token
}
