import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, readFile } from 'node:fs/promises'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { openGuiSession } from './browser-daemon-session.mjs'
import { runScenarios } from './scenarios/devices-kit.mjs'
export { runScenarios }

async function runLegacy() {
  // Browser run of Manage Devices on the kit History window (design DV0–DV7) against a REAL isolated daemon: the
  // sidebar's device rows filter the history, the labelled footer entry opens the sheet, the invitation is a row that outlives the
  // sheet, and `/devices` opens the sheet. The complete frontend is served by e2e/fixtures/history-full-app.tsx,
  // built for the target skin; only the native host is stubbed. The daemon is the History run's one (it seeds a
  // LAN phone, `kit-image`, whose image entry is the phone's only history):
  //   python3 apps/gui-go/e2e/history_kit_browser_daemon.py start --out <dir> --extra 0
  //   UC_UI_PLATFORM=macos node e2e/devices-kit-browser.mjs <dir>
  //   python3 apps/gui-go/e2e/history_kit_browser_daemon.py stop --out <dir>
  // Screenshots, daemon read-backs and assertions land in <dir> (devices-results.json).
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
  const output = path.resolve(process.argv[2] ?? 'target/e2e/devices-kit-browser')
  const platform = process.env.UC_UI_PLATFORM
  assert.ok(
    ['windows', 'macos', 'linux'].includes(platform),
    'UC_UI_PLATFORM=windows|macos|linux is required'
  )
  const daemon = JSON.parse(await readFile(path.join(output, 'daemon.json'), 'utf8'))
  const port = Number(process.env.UI_FIXTURE_PORT || 1463)
  await mkdir(output, { recursive: true })

  const server = spawn('node', ['e2e/visual-effects-server.mjs'], {
    env: {
      ...process.env,
      UI_FIXTURE_ENTRY: 'e2e/fixtures/history-full-app.tsx',
      UI_FIXTURE_PORT: String(port),
    },
    stdio: ['ignore', 'pipe', 'inherit'],
  })
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('fixture server did not start')), 180_000)
    server.stdout.on('data', chunk => {
      if (String(chunk).includes(`127.0.0.1:${port}`)) {
        clearTimeout(timer)
        resolve()
      }
    })
    server.on('exit', code => reject(new Error(`fixture server exited: ${code}`)))
  })

  // Each page context opens its own unlocked GUI session with the daemon's file token.
  async function guiSession() {
    const conn = JSON.parse(await readFile(daemon.conn, 'utf8'))
    return openGuiSession({ ...daemon, fileToken: conn.token })
  }
  let apiToken
  async function api(pathname) {
    apiToken ??= await guiSession()
    const response = await fetch(`${daemon.daemonUrl}${pathname}`, {
      headers: { Authorization: `Session ${apiToken}` },
    })
    return { status: response.status, body: response.ok ? await response.json() : null }
  }
  // The daemon's current invitation (the single source of truth the row presents).
  async function currentInvitation() {
    const { body } = await api('/v2/setup/state')
    return (body?.data ?? body)?.currentInvitation ?? null
  }

  const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
  try {
    await runScenarios({
      daemon,
      output,
      platform,
      currentInvitation,
      createContext: options => browser.newContext(options),
      preparePage: async page =>
        page.goto(
          `http://127.0.0.1:${port}/?${new URLSearchParams({ daemon: daemon.daemonUrl, token: await guiSession(), platform })}`
        ),
    })
  } finally {
    await browser.close()
    server.kill()
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await runLegacy()
