import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises'
import net from 'node:net'
import path from 'node:path'
import { openGuiSession } from '../../frontend/e2e/browser-daemon-session.mjs'
import { gradientPng, HISTORY_TEXTS } from '../../frontend/e2e/history-seed-data.mjs'
import { STARTUP_PASSPHRASE } from '../windows-startup-scenarios.mjs'

function parsePrivateJson(text) {
  try {
    return JSON.parse(text)
  } catch {
    throw new Error('Owned native fixture returned invalid JSON')
  }
}
async function request(daemon, route, body, method) {
  const response = await fetch(daemon.daemonUrl + route, {
    method,
    headers: {
      Authorization: `Session ${daemon.sessionToken}`,
      'Content-Type': 'application/json',
    },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(30_000),
  })
  assert.ok(response.ok, `owned daemon ${route.split('?')[0]} returned ${response.status}`)
  return parsePrivateJson(await response.text()).data
}

export async function openWindowsDaemon() {
  assert.equal(process.platform, 'win32')
  const home = path.resolve(process.env.UC_NATIVE_SANDBOX)
  const profile = process.env.UC_NATIVE_PROFILE
  assert.ok(home.toLowerCase().startsWith((path.resolve('target') + path.sep).toLowerCase()))
  assert.ok(path.basename(home).startsWith('uc-gui-go-'))
  assert.ok(profile.startsWith('gui-go-'))
  assert.equal(process.env.UC_PROFILE, profile)
  for (const key of ['UC_PORTABLE', 'UC_GUI_GO_ISOLATED', 'UC_DISABLE_SYSTEM_CLIPBOARD'])
    assert.equal(process.env[key], '1', `native seed ${key}`)
  const data = path.join(home, 'data')
  const connections = (await readdir(data, { recursive: true })).filter(
    name => path.basename(name) === 'daemon.conn'
  )
  assert.equal(connections.length, 1, 'one daemon connection inside the owned portable profile')
  const connection = parsePrivateJson(await readFile(path.join(data, connections[0]), 'utf8'))
  assert.ok(Number.isInteger(connection.port) && connection.port > 0)
  const daemon = {
    home,
    profile,
    daemonUrl: `http://127.0.0.1:${connection.port}`,
    fileToken: connection.token,
    passphrase: STARTUP_PASSPHRASE,
    seeded: 0,
  }
  daemon.sessionToken = await openGuiSession(daemon)
  await request(daemon, '/settings', { general: { language: 'en-US' } }, 'PUT')
  return daemon
}

async function waitForListener(port) {
  const deadline = Date.now() + 30_000
  while (Date.now() < deadline) {
    const ready = await new Promise(resolve => {
      const socket = net.createConnection({ host: '127.0.0.1', port })
      const finish = ready => {
        socket.destroy()
        resolve(ready)
      }
      socket.once('connect', () => finish(true))
      socket.once('error', () => finish(false))
      socket.setTimeout(500, () => finish(false))
    })
    if (ready) return
    await new Promise(resolve => setTimeout(resolve, 100))
  }
  throw new Error('Owned mobile listener did not become ready')
}

export async function seedWindowsHistory(daemon) {
  const cli = path.join(daemon.home, 'uniclip.exe')
  const command = (args, allowSendFailure = false) => {
    const result = spawnSync(cli, args, {
      env: process.env,
      encoding: 'utf8',
      windowsHide: true,
      timeout: 90_000,
    })
    const acceptedSend = allowSendFailure && result.stdout?.includes('snapshotHash')
    assert.ok(result.status === 0 || acceptedSend, `owned native CLI exited ${result.status}`)
    return result.stdout
  }
  const send = text => request(daemon, '/clipboard/dispatch', { text }, 'POST')
  for (let index = 0; index < 130; index++)
    await send(`Paging entry ${String(index).padStart(3, '0')}`)
  const folder = path.join(daemon.home, 'seed')
  await mkdir(folder)
  for (const [name, bytes] of [
    ['invoice-0921.txt', Buffer.from('Invoice 0921\nTotal: 42.00\n')],
    [
      'pixel.png',
      Buffer.from(
        '89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082',
        'hex'
      ),
    ],
  ]) {
    const file = path.join(folder, name)
    await writeFile(file, bytes)
    command(['--json', 'send', '--file', file], true)
  }
  const listener = net.createServer()
  await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve))
  const port = listener.address().port
  await new Promise(resolve => listener.close(resolve))
  const credentials = parsePrivateJson(
    command([
      '--json',
      'mobile',
      'setup',
      '--non-interactive',
      '--label',
      'kit-image',
      '--ip',
      '127.0.0.1',
      '--accept-network-risk',
      '--port',
      String(port),
    ])
  )
  await waitForListener(port)
  const authorization = `Basic ${Buffer.from(`${credentials.username}:${credentials.password}`).toString('base64')}`
  const put = async (route, body, type) => {
    const response = await fetch(`http://127.0.0.1:${port}${route}`, {
      method: 'PUT',
      headers: { Authorization: authorization, 'Content-Type': type },
      body,
      signal: AbortSignal.timeout(30_000),
    })
    assert.ok(response.ok, `owned mobile ingress returned ${response.status}`)
  }
  await put('/file/gradient-sample.png', gradientPng(), 'image/png')
  await put(
    '/SyncClipboard.json',
    JSON.stringify({
      type: 'Image',
      text: 'gradient-sample.png',
      dataName: 'gradient-sample.png',
      hasData: true,
    }),
    'application/json'
  )
  for (const text of HISTORY_TEXTS) {
    await send(text)
    await new Promise(resolve => setTimeout(resolve, 200))
  }
  const deadline = Date.now() + 60_000
  let count
  do {
    count = (await request(daemon, '/clipboard/entries?limit=200')).length
    if (count === 138) break
    await new Promise(resolve => setTimeout(resolve, 200))
  } while (Date.now() < deadline)
  assert.equal(count, 138, 'all original native fixture entries arrive through real ingress')
  daemon.seeded = count
}
