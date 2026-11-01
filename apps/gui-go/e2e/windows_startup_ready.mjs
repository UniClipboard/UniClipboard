// Run from the repository root after building target/debug/uniclipd.exe.
// Requires Go and Node with the built-in WebSocket API (Node 22+).
import assert from 'node:assert/strict'
import { spawn, spawnSync, execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { runStartupScenarios } from './windows-startup-scenarios.mjs'

assert.equal(process.platform, 'win32')
const root = fileURLToPath(new URL('../../../', import.meta.url))
const framework = process.argv.includes('--tester-army')
const publicOutput = path.join(root, 'target/tester-army-windows-product')
if (framework) {
  fs.rmSync(publicOutput, { recursive: true, force: true })
  fs.mkdirSync(publicOutput, { recursive: true })
}
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
  child.closed = new Promise(resolve => child.once('close', resolve))
  children.push(child)
  return child
}
let socket
let command
let failure = false
let host
let launchedEnv
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
  if (framework) {
    const built = path.join(root, 'target/gui-go/windows-e2e')
    const manifest = JSON.parse(fs.readFileSync(path.join(built, 'manifest.json'), 'utf8'))
    assert.equal(
      manifest.commit,
      execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim()
    )
    assert.equal(manifest.dirty, false, 'portable CLI must come from the clean current source')
    const bytes = fs.readFileSync(path.join(built, 'uniclip.exe'))
    assert.equal(createHash('sha256').update(bytes).digest('hex'), manifest.sha256['uniclip.exe'])
    fs.writeFileSync(path.join(sandbox, 'uniclip.exe'), bytes)
  }
  const vitePort = await freePort()
  const debugPort = await freePort()
  const env = {
    ...process.env,
    UNICLIPBOARD_ENV: 'development',
    UC_UI_PLATFORM: 'windows',
    UC_PROFILE: profile,
    UC_PORTABLE: '1',
    UC_GUI_GO_ISOLATED: '1',
    UC_GPUI_QUICK_PANEL: '0',
    UC_DISABLE_SYSTEM_CLIPBOARD: '1',
    UC_DEV_SERVER_PORT: String(vitePort),
    FRONTEND_DEVSERVER_URL: `http://127.0.0.1:${vitePort}`,
    UC_GUI_GO_E2E_PHASE: 'startup-observe',
    UC_GUI_GO_E2E_VISIBLE: framework ? '1' : '0',
    UC_GUI_GO_E2E_HIDDEN: framework ? '0' : '1',
    UC_GUI_GO_EXIT_MODE: 'full',
    UC_GUI_GO_EVIDENCE: path.join(sandbox, 'native.jsonl'),
    UC_GUI_GO_E2E_CONTROL_FILE: path.join(sandbox, 'control.txt'),
    UC_GUI_GO_E2E_BROWSER_ARGS: `--remote-debugging-port=${debugPort}`,
  }
  launchedEnv = env
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
  host = start(binary, [], { env, stdio: ['ignore', output, output] })
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
  if (framework) {
    const runner = start(
      process.execPath,
      [
        path.join(root, 'node_modules/e2e/dist/cli/bin.js'),
        'run',
        '--config',
        'e2e.windows-product.config.ts',
      ],
      {
        cwd: root,
        env: {
          ...env,
          E2E_TELEMETRY_DISABLED: '1',
          UC_NATIVE_ENDPOINT: `http://127.0.0.1:${debugPort}`,
          UC_NATIVE_URL: page.url,
          UC_NATIVE_TARGET_ID: page.id,
          UC_NATIVE_PROFILE: profile,
          UC_NATIVE_SANDBOX: sandbox,
        },
        stdio: 'inherit',
      }
    )
    const code = await runner.closed
    assert.equal(code, 0, 'Windows native product framework suite failed')
  } else {
    await runStartupScenarios({ waitFor, evaluate, evidence, delay })
  }
  console.log(JSON.stringify({ sandbox, evidence }, null, 2))
} catch (error) {
  failure = true
  throw error
} finally {
  fs.writeFileSync(path.join(sandbox, 'startup-ready.json'), JSON.stringify(evidence, null, 2))
  // Teardown must remain available after the framework releases its CDP connection.
  fs.appendFileSync(path.join(sandbox, 'control.txt'), 'exit\n')
  socket?.close()
  const cleanup = {
    systemClipboardDisabled: launchedEnv?.UC_DISABLE_SYSTEM_CLIPBOARD === '1',
    guiIsolated: launchedEnv?.UC_GUI_GO_ISOLATED === '1',
    portable: launchedEnv?.UC_PORTABLE === '1',
    hostExitedNormally: false,
    daemonExited: false,
    sandboxRemoved: false,
    forced: false,
  }
  try {
    // Only connection files inside this run's portable sandbox can nominate a daemon.
    const connRoot = path.join(sandbox, 'data', `app.uniclipboard.desktop-${profile}`)
    const daemonPids = new Set()
    for (const name of ['daemon.conn', 'daemon-startup.conn']) {
      const connPath = path.join(connRoot, name)
      if (fs.existsSync(connPath)) daemonPids.add(JSON.parse(fs.readFileSync(connPath, 'utf8')).pid)
    }
    if (host)
      cleanup.hostExitedNormally = await Promise.race([
        host.closed.then(code => code === 0),
        delay(45000).then(() => false),
      ])
    for (const pid of daemonPids) {
      try {
        process.kill(pid, 0)
      } catch {
        continue
      }
      // A PID from a connection file is not sufficient ownership proof.
      const actual = execFileSync(
        'powershell.exe',
        [
          '-NoProfile',
          '-NonInteractive',
          '-Command',
          `(Get-Process -Id ${pid} -ErrorAction Stop).Path`,
        ],
        { encoding: 'utf8', timeout: 5000 }
      ).trim()
      assert.equal(
        path.resolve(actual).toLowerCase(),
        path.join(sandbox, 'uniclipd.exe').toLowerCase()
      )
      process.kill(pid)
      cleanup.forced = true
    }
  } catch (error) {
    cleanup.errorType = error.name
  } finally {
    for (const child of children.reverse()) {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill()
        if (child === host) cleanup.forced = true
      }
      await Promise.race([child.closed, delay(5000)])
    }
  }
  try {
    const remaining = execFileSync(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `@(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith('${sandbox.replaceAll("'", "''")}' + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) }).Count`,
      ],
      { encoding: 'utf8', timeout: 5000 }
    ).trim()
    cleanup.daemonExited = remaining === '0' && !cleanup.errorType
  } catch (error) {
    cleanup.errorType = error.name
  }
  try {
    cleanup.controlExitAcknowledged = fs
      .readFileSync(path.join(sandbox, 'native.jsonl'), 'utf8')
      .split('\n')
      .filter(Boolean)
      .some(line => {
        const value = JSON.parse(line)
        return value.step === 'control-exit' && value.ok === true
      })
  } catch {
    cleanup.controlExitAcknowledged = false
  }
  if (framework) {
    // Export public evidence only; never copy daemon connection files, private stores or raw logs.
    const hash = file =>
      fs.existsSync(file) ? createHash('sha256').update(fs.readFileSync(file)).digest('hex') : null
    const sourceFiles = [
      'apps/gui-go/e2e/windows_startup_ready.mjs',
      'apps/gui-go/e2e/windows-startup-scenarios.mjs',
      'apps/gui-go/e2e/tester-army-native/windows-product.e2e.mjs',
      'scripts/gui-e2e.mjs',
      'apps/gui-go/frontend/e2e/tester-army/build-lease.mjs',
      'apps/gui-go/frontend/e2e/scenarios/history-kit.mjs',
      'e2e.windows-product.config.ts',
      'apps/gui-go/e2e/tester-army-native/windows-history-seed.mjs',
      'apps/gui-go/frontend/e2e/history-bounded-grid.mjs',
      'apps/gui-go/frontend/e2e/history-seed-data.mjs',
      'Cargo.lock',
      'bun.lock',
    ]
    const identity = {
      head: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
      dirty:
        execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).trim() !==
        '',
      sourceSha256: Object.fromEntries(
        sourceFiles.map(file => [file, hash(path.join(root, file))])
      ),
      binariesSha256: Object.fromEntries(
        ['gui-go.exe', 'uniclipd.exe', 'uniclip.exe'].map(file => [
          file,
          hash(path.join(sandbox, file)),
        ])
      ),
    }
    if (cleanup.daemonExited) {
      for (let retry = 0; retry < 100; retry++) {
        try {
          fs.rmSync(sandbox, { recursive: true })
          cleanup.sandboxRemoved = true
          break
        } catch {
          await delay(100)
        }
      }
    }
    fs.writeFileSync(
      path.join(publicOutput, 'isolation.json'),
      JSON.stringify({ ...identity, ...cleanup }, null, 2)
    )
    if (!failure) {
      assert.equal(
        cleanup.controlExitAcknowledged,
        true,
        'the product must acknowledge the owned exit request'
      )
      assert.equal(cleanup.hostExitedNormally, true, 'native host must quit normally')
      assert.equal(cleanup.forced, false, 'forced termination is not successful native cleanup')
      assert.equal(cleanup.daemonExited, true, 'no owned product processes may remain')
      assert.equal(cleanup.sandboxRemoved, true, 'private portable profile must be removed')
    }
  }
  console.log(framework ? `Public evidence: ${publicOutput}` : `Evidence retained at ${sandbox}`)
}
