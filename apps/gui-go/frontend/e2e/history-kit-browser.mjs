import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, readFile } from 'node:fs/promises'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { openGuiSession } from './browser-daemon-session.mjs'
import { runScenarios } from './scenarios/history-kit.mjs'
export { runScenarios }

async function runLegacy() {
  // Browser run of the kit History page against a REAL isolated daemon (DOM and keyboard in Chromium; not a native
  // WebView2 acceptance). The complete frontend is served by e2e/fixtures/history-full-app.tsx, built for the target
  // skin; only the native host is stubbed. Start the daemon first:
  //   python3 apps/gui-go/e2e/history_kit_browser_daemon.py start --out <dir>
  //   UC_UI_PLATFORM=windows node e2e/history-kit-browser.mjs <dir>
  //   python3 apps/gui-go/e2e/history_kit_browser_daemon.py stop --out <dir>
  // Screenshots and assertions land in <dir>.
  //
  // With HISTORY_KIT_CDP=<http://host:port> the same steps drive the REAL app instead: the Wails WebView (WebView2 on
  // Windows, started with --remote-debugging-port) is attached over the DevTools protocol, no fixture is served, input
  // is real CDP input, and colour scheme and window sizes are page emulation (Emulation.*), not native window changes.
  // apps/gui-go/e2e/windows_history_kit_remote.py runs it that way against a Windows machine.
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
  const output = path.resolve(process.argv[2] ?? 'target/e2e/history-kit-browser')
  const platform = process.env.UC_UI_PLATFORM
  assert.ok(
    ['windows', 'macos', 'linux'].includes(platform),
    'UC_UI_PLATFORM=windows|macos|linux is required'
  )
  const daemon = JSON.parse(await readFile(path.join(output, 'daemon.json'), 'utf8'))
  // The seed sends `Paging entry 000`..`NNN` first, then 8 named entries (history-kit-seed.mjs).
  const port = Number(process.env.UI_FIXTURE_PORT || 1462)
  await mkdir(output, { recursive: true })

  const cdp = process.env.HISTORY_KIT_CDP

  // GUI session tokens live five minutes and the fixture hands the same one out on refresh, so each page context
  // opens its own session with the daemon's file token (daemon.conn in the isolated data dir).
  async function guiSession() {
    const conn = JSON.parse(await readFile(daemon.conn, 'utf8'))
    return openGuiSession({ ...daemon, fileToken: conn.token })
  }
  async function pageUrl() {
    return `http://127.0.0.1:${port}/?${new URLSearchParams({ daemon: daemon.daemonUrl, token: await guiSession(), platform })}`
  }
  let server
  let browser
  try {
    server = cdp
      ? null
      : spawn('node', ['e2e/visual-effects-server.mjs'], {
          env: {
            ...process.env,
            UI_FIXTURE_ENTRY: 'e2e/fixtures/history-full-app.tsx',
            UI_FIXTURE_PORT: String(port),
          },
          stdio: ['ignore', 'pipe', 'inherit'],
        })
    if (server) {
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('fixture server did not start')), 180_000)
        server.stdout.on('data', chunk => {
          if (String(chunk).includes(`127.0.0.1:${port}`)) {
            clearTimeout(timer)
            resolve()
          }
        })
        server.once('exit', code => {
          clearTimeout(timer)
          reject(new Error(`fixture server exited: ${code}`))
        })
        server.once('error', error => {
          clearTimeout(timer)
          reject(error)
        })
      })
    }
    browser = cdp
      ? await chromium.connectOverCDP(cdp)
      : await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
    await runScenarios({
      daemon,
      output,
      platform,
      cdp,
      browser,
      createContext: options => browser.newContext(options),
      guiSession,
      preparePage: async page => page.goto(await pageUrl()),
    })
  } finally {
    try {
      await browser?.close()
    } finally {
      server?.kill()
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await runLegacy()
