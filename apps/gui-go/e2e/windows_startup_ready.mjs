// Run from the repository root after building target/debug/uniclipd.exe.
// Requires Go and Node with the built-in WebSocket API (Node 22+).
import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

assert.equal(process.platform, 'win32')
const root = fileURLToPath(new URL('../../../', import.meta.url))
const sandboxParent = process.argv.includes('--long-path')
  ? path.join(root, 'target', 'uc-gui-go-portable-long-path-'.repeat(3))
  : path.join(root, 'target')
fs.mkdirSync(sandboxParent, { recursive: true })
const sandbox = fs.mkdtempSync(path.join(sandboxParent, 'uc-gui-go-'))
const profile = `gui-go-${Date.now().toString(36)}`
const children = []
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
async function waitFor(probe, label, timeout = 60000) {
  const end = Date.now() + timeout
  while (Date.now() < end) {
    const value = await probe()
    if (value) return value
    await delay(250)
  }
  throw new Error(`Timed out: ${label}`)
}
async function freePort() {
  const server = net.createServer()
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const port = server.address().port
  await new Promise(resolve => server.close(resolve))
  return port
}
function start(command, args, options = {}) {
  const child = spawn(command, args, { windowsHide: true, ...options })
  children.push(child)
  return child
}
let socket
let command
const evidence = []
try {
  const binary = path.join(sandbox, 'gui-go.exe')
  const build = spawnSync('go', ['build', '-tags', 'e2e', '-o', binary, '.'], {
    cwd: path.join(root, 'apps/gui-go'),
    windowsHide: true,
    stdio: 'inherit',
  })
  assert.equal(build.status, 0, 'Go E2E host build failed')
  fs.copyFileSync(path.join(root, 'target/debug/uniclipd.exe'), path.join(sandbox, 'uniclipd.exe'))
  const vitePort = await freePort()
  const debugPort = await freePort()
  const env = {
    ...process.env,
    UNICLIPBOARD_ENV: 'development',
    UC_PROFILE: profile,
    UC_PORTABLE: '1',
    UC_GUI_GO_ISOLATED: '1',
    UC_DISABLE_SYSTEM_CLIPBOARD: '1',
    UC_DEV_SERVER_PORT: String(vitePort),
    FRONTEND_DEVSERVER_URL: `http://127.0.0.1:${vitePort}`,
    UC_GUI_GO_E2E_PHASE: 'startup-observe',
    UC_GUI_GO_E2E_VISIBLE: '0',
    UC_GUI_GO_E2E_HIDDEN: '1',
    UC_GUI_GO_EXIT_MODE: 'full',
    UC_GUI_GO_EVIDENCE: path.join(sandbox, 'native.jsonl'),
    UC_GUI_GO_E2E_BROWSER_ARGS: `--remote-debugging-port=${debugPort}`,
  }
  start(process.execPath, [path.join(root, 'node_modules/vite/bin/vite.js')], {
    cwd: path.join(root, 'apps/gui-go/frontend'),
    env,
    stdio: 'ignore',
  })
  await waitFor(async () => {
    try {
      return (await fetch(env.FRONTEND_DEVSERVER_URL)).ok
    } catch {
      return false
    }
  }, 'Vite ready')
  const output = fs.openSync(path.join(sandbox, 'host.log'), 'w')
  const host = start(binary, [], { env, stdio: ['ignore', output, output] })
  fs.closeSync(output)
  const page = await waitFor(async () => {
    if (host.exitCode !== null) throw new Error(`Host exited: ${host.exitCode}`)
    const startupPath = path.join(
      sandbox,
      'data',
      `app.uniclipboard.desktop-${profile}`,
      'daemon-startup.conn'
    )
    if (fs.existsSync(startupPath)) {
      const conn = JSON.parse(fs.readFileSync(startupPath, 'utf8'))
      const response = await fetch(`http://${conn.host}:${conn.port}/startup`, {
        headers: { Authorization: `Bearer ${conn.token}` },
        signal: AbortSignal.timeout(3000),
      })
      if (response.ok) {
        const status = await response.json()
        if (status.service_failed || ['failed', 'interrupted'].includes(status.progress.state)) {
          evidence.push({ step: 'daemon-startup-failed', status })
          throw new Error(
            `Daemon startup failed before WebView creation: ${JSON.stringify(status.progress.failure)}`
          )
        }
      }
    }
    try {
      return (await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json()).find(
        item => item.type === 'page' && item.url.endsWith('/')
      )
    } catch {
      return null
    }
  }, 'native WebView debugging endpoint')
  socket = new WebSocket(page.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    socket.onopen = resolve
    socket.onerror = reject
  })
  let sequence = 0
  const pending = new Map()
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data)
    if (message.id) {
      const waiter = pending.get(message.id)
      if (!waiter) return
      pending.delete(message.id)
      clearTimeout(waiter.timer)
      if (message.error) waiter.reject(new Error(message.error.message))
      else waiter.resolve(message.result)
    }
  }
  command = (method, params) =>
    new Promise((resolve, reject) => {
      const id = ++sequence
      const timer = setTimeout(() => {
        pending.delete(id)
        reject(new Error(`CDP command timed out: ${method}`))
      }, 10000)
      pending.set(id, { resolve, reject, timer })
      socket.send(JSON.stringify({ id, method, params }))
    })
  async function evaluate(expression) {
    const result = await command('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    })
    if (result.exceptionDetails)
      throw new Error(result.exceptionDetails.text + ': ' + result.result.description)
    return result.result.value
  }
  const status = await waitFor(
    () => evaluate(`import('/src/lib/ipc.ts').then(m => m.commands.getDaemonStartupStatus())`),
    'startup status'
  )
  assert.equal(status.service_ready, true)
  evidence.push({
    step: 'native-startup-status',
    serviceReady: status.service_ready,
    state: status.progress.state,
  })
  await waitFor(
    () => evaluate(`!!document.querySelector('[data-testid="setup-entry-create"]')`),
    'setup screen leaves startup'
  )
  evidence.push({ step: 'setup-screen', ok: true })
  await evaluate(`document.querySelector('[data-testid="setup-entry-create"]').click()`)
  await waitFor(() => evaluate(`!!document.querySelector('#device-name')`), 'setup form')
  await evaluate(`(() => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    for (const [selector, value] of [['#device-name','synthetic-startup-device'],['#pass1','synthetic-startup-passphrase'],['#pass2','synthetic-startup-passphrase']]) {
      const input = document.querySelector(selector); setter.call(input,value); input.dispatchEvent(new Event('input',{bubbles:true}));
    }
  })()`)
  await delay(300)
  await evaluate(`document.querySelector('[data-testid="setup-initialize-submit"]').click()`)
  await waitFor(
    () =>
      evaluate(
        `!!document.querySelector('[data-testid="setup-complete-later"]') || location.pathname.startsWith('/history')`
      ),
    'setup completed'
  )
  await evaluate(`document.querySelector('[data-testid="setup-complete-later"]')?.click()`)
  await waitFor(
    () =>
      evaluate(
        `location.pathname.startsWith('/history') && !!document.querySelector('a[href="/settings"]')`
      ),
    'authenticated history screen'
  )
  evidence.push({ step: 'authenticated-history-screen', ok: true })
  console.log(JSON.stringify({ sandbox, evidence }, null, 2))
} finally {
  fs.writeFileSync(path.join(sandbox, 'startup-ready.json'), JSON.stringify(evidence, null, 2))
  if (command && socket?.readyState === WebSocket.OPEN) {
    await Promise.race([
      command('Runtime.evaluate', {
        expression: `import('/node_modules/@wailsio/runtime/dist/index.js').then(m => m.Call.ByName('main.EvidenceService.Control','exit'))`,
        awaitPromise: true,
      }).catch(() => {}),
      delay(3000),
    ])
    socket.close()
  }
  // Only stop children created by this run, including the sandbox's daemon.
  const connRoot = path.join(sandbox, 'data', `app.uniclipboard.desktop-${profile}`)
  const daemonPids = new Set()
  for (const name of ['daemon.conn', 'daemon-startup.conn']) {
    const connPath = path.join(connRoot, name)
    if (fs.existsSync(connPath)) {
      daemonPids.add(JSON.parse(fs.readFileSync(connPath, 'utf8')).pid)
    }
  }
  for (const pid of daemonPids) {
    try {
      process.kill(pid)
    } catch {
      // The host may already have stopped its daemon during the full quit.
    }
  }
  for (const child of children.reverse()) if (child.exitCode === null) child.kill()
  console.log(`Evidence retained at ${sandbox}`)
}
