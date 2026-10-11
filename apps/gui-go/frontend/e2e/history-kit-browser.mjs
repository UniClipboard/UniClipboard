import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'

// Browser run of the kit History page against a REAL isolated daemon (DOM and keyboard in Chromium; not a native
// WebView2 acceptance). The complete frontend is served by e2e/fixtures/history-full-app.tsx, built for the target
// skin; only the native host is stubbed. Start the daemon first:
//   python3 apps/gui-go/e2e/history_kit_browser_daemon.py start --out <dir>
//   UC_UI_PLATFORM=windows node e2e/history-kit-browser.mjs <dir>
//   python3 apps/gui-go/e2e/history_kit_browser_daemon.py stop --out <dir>
// Screenshots and assertions land in <dir>.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const output = path.resolve(process.argv[2] ?? 'target/e2e/history-kit-browser')
const platform = process.env.UC_UI_PLATFORM
assert.ok(platform === 'windows' || platform === 'macos', 'UC_UI_PLATFORM=windows|macos is required')
const daemon = JSON.parse(await readFile(path.join(output, 'daemon.json'), 'utf8'))
const port = Number(process.env.UI_FIXTURE_PORT || 1462)
await mkdir(output, { recursive: true })

const server = spawn('node', ['e2e/visual-effects-server.mjs'], {
  env: { ...process.env, UI_FIXTURE_ENTRY: 'e2e/fixtures/history-full-app.tsx', UI_FIXTURE_PORT: String(port) },
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

// GUI session tokens live five minutes and the fixture hands the same one out on refresh, so each page context
// opens its own session with the daemon's file token (daemon.conn in the isolated data dir).
async function guiSession() {
  const conn = JSON.parse(await readFile(daemon.conn, 'utf8'))
  const response = await fetch(`${daemon.daemonUrl}/auth/connect`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${conn.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ pid: process.pid, clientType: 'gui' }),
  })
  assert.ok(response.ok, `auth/connect: ${response.status}`)
  const token = (await response.json()).data.sessionToken
  // A GUI session starts content-locked; unlock it with the space passphrase, as the unlock screen would.
  const unlock = await fetch(`${daemon.daemonUrl}/content-lock/unlock`, {
    method: 'POST',
    headers: { Authorization: `Session ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ passphrase: daemon.passphrase }),
  })
  assert.ok(unlock.ok, `content-lock/unlock: ${unlock.status} ${await unlock.text()}`)
  return token
}
async function pageUrl() {
  return `http://127.0.0.1:${port}/?${new URLSearchParams({ daemon: daemon.daemonUrl, token: await guiSession(), platform })}`
}
const results = { platform, profile: daemon.profile, steps: [] }
const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
let page
async function step(name, run) {
  try {
    const detail = await run()
    results.steps.push({ name, ok: true, detail })
  } catch (error) {
    results.steps.push({ name, ok: false, error: String(error?.stack ?? error) })
    await page?.screenshot({ path: path.join(output, `failed-${name}.png`) }).catch(() => {})
  }
}
// App-level prompts that are not part of History (the first-run diagnostics consent) are answered the way a user
// would, inside the isolated profile.
async function dismissStartupPrompts() {
  const consent = page.getByRole('alertdialog').getByRole('button', { name: 'Disable' })
  await consent.waitFor({ timeout: 5000 }).catch(() => {})
  if (await consent.isVisible()) {
    await page.screenshot({ path: path.join(output, 'startup-prompt.png') })
    await consent.click({ timeout: 5000 })
    await consent.waitFor({ state: 'hidden' })
  }
}
async function shot(name) {
  await page.waitForTimeout(300)
  await page.screenshot({ path: path.join(output, `${name}.png`) })
}

try {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    reducedMotion: 'reduce',
    colorScheme: 'light',
  })
  page = await context.newPage()
  await page.goto(await pageUrl())
  await step('page-loads', async () => {
    await page.locator('[data-item-id]').first().waitFor({ timeout: 60_000 })
    await dismissStartupPrompts()
    await shot('default-light')
    return {
      rows: await page.locator('[data-item-id]').count(),
      layout: await page.evaluate(() => {
        const chain = []
        for (let el = document.querySelector('[data-item-id]'); el; el = el.parentElement) {
          const r = el.getBoundingClientRect()
          chain.push(`${el.tagName.toLowerCase()}${el.id ? '#' + el.id : ''}.${String(el.className).slice(0, 40)} h=${Math.round(r.height)}`)
        }
        return chain
      }),
    }
  })
} finally {
  results.pageErrors = await page?.evaluate(() => window.__ucPageErrors ?? []).catch(() => null)
  results.nativeCalls = await page?.evaluate(() => window.__ucNativeCalls ?? []).catch(() => null)
  await browser.close()
  server.kill()
  results.passed = results.steps.every(s => s.ok)
  await writeFile(path.join(output, 'browser-results.json'), `${JSON.stringify(results, null, 2)}\n`)
  console.log(JSON.stringify({ passed: results.passed, steps: results.steps.map(s => [s.name, s.ok]) }))
}
assert.ok(results.passed, 'browser checks failed; see browser-results.json')
