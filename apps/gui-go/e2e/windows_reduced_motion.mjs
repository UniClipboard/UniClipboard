// Run on Windows with Node 22+, Go, and a matching daemon:
// node apps/gui-go/e2e/windows_reduced_motion.mjs --daemon <uniclipd.exe> [--embedded] [--expect-failure]
// Reads native WebView DOM and exception events. Does not evaluate JavaScript or call business APIs.
import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

assert.equal(process.platform, 'win32', 'Run this acceptance check in Windows')
process.env.UC_UI_PLATFORM = 'windows'
process.env.VITE_SENTRY_DSN = ''
process.env.SENTRY_AUTH_TOKEN = ''
process.env.VITE_SENTRY_PROJECT = ''
const root = fileURLToPath(new URL('../../../', import.meta.url))
const option = name => process.argv[process.argv.indexOf(name) + 1]
assert.ok(process.argv.includes('--daemon'), '--daemon is required')
const daemon = path.resolve(option('--daemon'))
const embedded = process.argv.includes('--embedded')
const expectFailure = process.argv.includes('--expect-failure')
const parent = path.join(root, 'target')
fs.mkdirSync(parent, { recursive: true })
const sandbox = fs.mkdtempSync(path.join(parent, 'uc-gui-go-motion-'))
const binary = path.join(sandbox, 'gui-go.exe')
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex')
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
async function waitFor(probe, label, timeout = 90000) {
  const end = Date.now() + timeout
  while (Date.now() < end) {
    const result = await probe()
    if (result) return result
    await delay(100)
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
const build = spawnSync('go', ['build', '-buildvcs=false', '-tags', 'e2e', '-o', binary, '.'], {
  cwd: path.join(root, 'apps/gui-go'),
  stdio: 'inherit',
  windowsHide: true,
})
assert.equal(build.status, 0, 'Native E2E host build failed')
fs.copyFileSync(daemon, path.join(sandbox, 'uniclipd.exe'))
const receipt = {
  startedAt: new Date().toISOString(),
  sandbox,
  frontend: embedded ? 'embedded production bundle' : 'Vite development',
  hostSha256: hash(binary),
  daemonSha256: hash(daemon),
  progressBarSha256: hash(
    path.join(root, 'apps/gui-go/frontend/src/ui-kit/skins/windows/ProgressBar.tsx')
  ),
  expectFailure,
  cases: [],
}
let vite
try {
  const vitePort = await freePort()
  process.env.UC_DEV_SERVER_PORT = String(vitePort)
  if (!embedded) {
    const { createServer } = await import('vite')
    vite = await createServer({
      root: path.join(root, 'apps/gui-go/frontend'),
      configFile: path.join(root, 'apps/gui-go/frontend/vite.config.ts'),
      server: { host: '127.0.0.1', port: vitePort, strictPort: true },
    })
    await vite.listen()
  }
  for (const theme of expectFailure ? ['light'] : ['light', 'dark']) {
    const profile = `gui-go-motion-${theme}-${Date.now().toString(36)}`
    const debugPort = await freePort()
    const control = path.join(sandbox, `${theme}.control`)
    fs.writeFileSync(control, '')
    const evidence = { theme, profile, debugPort, exceptions: [], setupReached: false }
    receipt.cases.push(evidence)
    const env = {
      ...process.env,
      UNICLIPBOARD_ENV: 'development',
      UC_PROFILE: profile,
      UC_PORTABLE: '1',
      UC_GUI_GO_ISOLATED: '1',
      UC_DISABLE_SYSTEM_CLIPBOARD: '1',
      UC_UI_PLATFORM: 'windows',
      VITE_SENTRY_DSN: '',
      SENTRY_AUTH_TOKEN: '',
      VITE_SENTRY_PROJECT: '',
      FRONTEND_DEVSERVER_URL: embedded ? '' : `http://127.0.0.1:${vitePort}`,
      UC_DEV_SERVER_PORT: String(vitePort),
      UC_GUI_GO_E2E_PHASE: 'startup-observe',
      UC_GUI_GO_E2E_VISIBLE: '1',
      UC_GUI_GO_E2E_CONTROL_FILE: control,
      UC_GUI_GO_EXIT_MODE: 'full',
      UC_GUI_GO_EVIDENCE: path.join(sandbox, `${theme}.native.jsonl`),
      UC_GUI_GO_E2E_BROWSER_ARGS: `--remote-debugging-port=${debugPort} --remote-debugging-address=127.0.0.1 --force-prefers-reduced-motion`,
    }
    const output = fs.openSync(path.join(sandbox, `${theme}.host.log`), 'w')
    const host = spawn(binary, [], {
      cwd: root,
      env,
      stdio: ['ignore', output, output],
      windowsHide: true,
    })
    fs.closeSync(output)
    evidence.pid = host.pid
    let socket
    try {
      const page = await waitFor(async () => {
        assert.equal(host.exitCode, null, 'Host exited before observation')
        try {
          const pages = await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json()
          return pages.find(
            item =>
              item.type === 'page' && !item.url.includes('quick-panel') && item.url.endsWith('/')
          )
        } catch {
          return null
        }
      }, 'native WebView endpoint')
      evidence.pageUrl = page.url
      const listeners = spawnSync(
        'powershell.exe',
        [
          '-NoProfile',
          '-Command',
          `Get-NetTCPConnection -State Listen -LocalPort ${debugPort} | Select-Object LocalAddress,LocalPort | ConvertTo-Json -Compress`,
        ],
        { encoding: 'utf8', windowsHide: true }
      )
      assert.equal(listeners.status, 0, 'Could not verify diagnostic listener')
      evidence.listeners = [JSON.parse(listeners.stdout)].flat()
      assert.ok(evidence.listeners.length > 0)
      assert.ok(
        evidence.listeners.every(item => ['127.0.0.1', '::1'].includes(item.LocalAddress)),
        'Diagnostic listener must stay on loopback'
      )
      socket = new WebSocket(page.webSocketDebuggerUrl)
      await new Promise((resolve, reject) => {
        socket.onopen = resolve
        socket.onerror = reject
      })
      let sequence = 0
      const pending = new Map()
      socket.onmessage = ({ data }) => {
        const message = JSON.parse(data)
        if (message.method === 'Runtime.exceptionThrown') {
          const detail = message.params.exceptionDetails
          evidence.exceptions.push({
            text: detail.text,
            description: detail.exception?.description,
            stack: detail.stackTrace,
          })
        }
        const waiter = pending.get(message.id)
        if (!waiter) return
        pending.delete(message.id)
        clearTimeout(waiter.timer)
        if (message.error) waiter.reject(new Error(message.error.message))
        else waiter.resolve(message.result)
      }
      const command = (method, params = {}) =>
        new Promise((resolve, reject) => {
          const id = ++sequence
          const timer = setTimeout(() => {
            pending.delete(id)
            reject(new Error(`CDP timeout: ${method}`))
          }, 10000)
          pending.set(id, { resolve, reject, timer })
          socket.send(JSON.stringify({ id, method, params }))
        })
      await command('Runtime.enable')
      await command('DOM.enable')
      await command('Emulation.setEmulatedMedia', {
        features: [
          { name: 'prefers-reduced-motion', value: 'reduce' },
          { name: 'prefers-color-scheme', value: theme },
        ],
      })
      const state = async () => {
        const { root: document } = await command('DOM.getDocument')
        const { outerHTML } = await command('DOM.getOuterHTML', { nodeId: document.nodeId })
        return outerHTML
      }
      let html = ''
      await waitFor(
        async () => {
          html = await state()
          if (expectFailure)
            return (
              html.includes('Something went wrong.') &&
              evidence.exceptions.some(e => e.description?.includes('infinite target effect end'))
            )
          assert.ok(!html.includes('Something went wrong.'), 'The React error boundary is visible')
          assert.equal(evidence.exceptions.length, 0, 'Native WebView raised an exception')
          evidence.setupReached = html.includes('data-testid="setup-entry-create"')
          return (
            evidence.setupReached &&
            html.includes(`data-color-scheme="${theme}"`) &&
            html.includes('data-uc-reduce-motion="true"')
          )
        },
        expectFailure ? 'original animation failure' : 'setup screen under reduced motion'
      )
      // Keep observing after startup to catch delayed effect failures.
      await delay(2000)
      html = await state()
      fs.writeFileSync(path.join(sandbox, `${theme}.dom.html`), html)
      const screenshot = await command('Page.captureScreenshot', { format: 'png' })
      fs.writeFileSync(path.join(sandbox, `${theme}.png`), Buffer.from(screenshot.data, 'base64'))
      evidence.reducedMotion = html.includes('data-uc-reduce-motion="true"')
      evidence.actualTheme = html.match(/data-color-scheme="([^"]+)"/)?.[1]
      evidence.fallbackVisible = html.includes('Something went wrong.')
      if (!expectFailure) {
        assert.equal(evidence.exceptions.length, 0)
        assert.equal(evidence.fallbackVisible, false)
        assert.ok(html.includes('data-testid="setup-entry-create"'))
      }
      evidence.passed = true
    } finally {
      socket?.close()
      // Existing E2E-only control runs the normal host quit path and stops its own daemon.
      fs.appendFileSync(control, 'exit\n')
      await waitFor(() => host.exitCode !== null, 'normal task-owned host exit', 20000)
      evidence.exitCode = host.exitCode
      await waitFor(
        async () => {
          try {
            await fetch(`http://127.0.0.1:${debugPort}/json/list`, {
              signal: AbortSignal.timeout(1000),
            })
            return false
          } catch {
            return true
          }
        },
        'diagnostic listener closed',
        5000
      )
      evidence.diagnosticPortClosed = true
    }
  }
  receipt.passed = true
} catch (error) {
  receipt.passed = false
  receipt.failure = error.stack
  process.exitCode = 1
} finally {
  await vite?.close()
  receipt.finishedAt = new Date().toISOString()
  fs.writeFileSync(path.join(sandbox, 'result.json'), JSON.stringify(receipt, null, 2))
  console.log(JSON.stringify(receipt, null, 2))
  console.log(`Evidence retained at ${sandbox}`)
}
