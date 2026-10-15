import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'

// Browser run of Manage Devices on the kit History window (design DV0–DV7) against a REAL isolated daemon: the
// sidebar's device rows filter the history, the gear opens the sheet, the invitation is a row that outlives the
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
const PHONE = 'kit-image'
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
  const response = await fetch(`${daemon.daemonUrl}/auth/connect`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${conn.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ pid: process.pid, clientType: 'gui' }),
  })
  assert.ok(response.ok, `auth/connect: ${response.status}`)
  const token = (await response.json()).data.sessionToken
  const unlock = await fetch(`${daemon.daemonUrl}/content-lock/unlock`, {
    method: 'POST',
    headers: { Authorization: `Session ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ passphrase: daemon.passphrase }),
  })
  assert.ok(unlock.ok, `content-lock/unlock: ${unlock.status} ${await unlock.text()}`)
  return token
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

const results = {
  head: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
  dirty: execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim() !== '',
  platform,
  mode: 'browser-fixture',
  profile: daemon.profile,
  steps: [],
  consoleErrors: [],
}
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
async function shot(name) {
  await page.waitForTimeout(400)
  await page.screenshot({ path: path.join(output, `${platform}-${name}.png`) })
}
async function dismissStartupPrompts() {
  const consent = page.getByRole('alertdialog').getByRole('button', { name: 'Disable' })
  await consent.waitFor({ timeout: 5000 }).catch(() => {})
  if (await consent.isVisible()) {
    await consent.click({ timeout: 5000 })
    await consent.waitFor({ state: 'hidden' })
  }
}
const rows = () => page.locator('[data-item-id]')
async function rowCountBecomes(expected, timeout = 10_000) {
  const end = Date.now() + timeout
  let count = await rows().count()
  while (count !== expected && Date.now() < end) {
    await page.waitForTimeout(200)
    count = await rows().count()
  }
  return count
}
async function newPage(colorScheme, reducedMotion = 'reduce') {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    reducedMotion,
    colorScheme,
  })
  page = await context.newPage()
  page.on('pageerror', error => results.consoleErrors.push(error.message))
  await page.goto(
    `http://127.0.0.1:${port}/?${new URLSearchParams({ daemon: daemon.daemonUrl, token: await guiSession(), platform })}`
  )
  await rows().first().waitFor({ timeout: 60_000 })
  await dismissStartupPrompts()
}
const sidebar = () => page.getByRole('navigation', { name: 'Library' })
const sheet = () => page.getByRole('dialog', { name: 'Devices' })
const deviceList = () => sheet().getByRole('listbox', { name: 'Devices' })
const option = name => deviceList().getByRole('option', { name: new RegExp(`^${name}`) })
const openSheet = async () => {
  await sidebar().getByRole('button', { name: 'Manage Devices…' }).click()
  await deviceList().waitFor({ timeout: 15_000 })
}
const closeSheet = async () => {
  await sheet().getByRole('button', { name: 'Done', exact: true }).click()
  await sheet().waitFor({ state: 'hidden' })
}

try {
  await newPage('light')

  await step('sidebar-device-rows', async () => {
    const phone = sidebar().getByRole('button', { name: new RegExp(`^${PHONE}`) })
    await phone.waitFor({ timeout: 30_000 })
    await shot('dv0-sidebar')
    return { phoneRow: await phone.innerText() }
  })

  await step('sidebar-device-filters-history', async () => {
    const all = await rows().count()
    const phone = sidebar().getByRole('button', { name: new RegExp(`^${PHONE}`) })
    await phone.click()
    // The phone sent exactly one entry (the image) in the seeded history.
    const filtered = await rowCountBecomes(1)
    assert.equal(filtered, 1, `history filtered to ${PHONE}: ${filtered} rows`)
    assert.equal(
      await phone.getAttribute('aria-current'),
      'page',
      'the device row is the selection'
    )
    await shot('dv0-sidebar-filtered')
    // Choosing it again clears the filter.
    await phone.click()
    const cleared = await rowCountBecomes(all)
    assert.equal(cleared, all, 'the filter is cleared by choosing the device again')
    return { all, filtered, cleared }
  })

  await step('sidebar-devices-collapse', async () => {
    const phone = sidebar().getByRole('button', { name: new RegExp(`^${PHONE}`) })
    await sidebar().getByRole('button', { name: 'Hide Devices' }).click()
    await phone.waitFor({ state: 'hidden' })
    const stored = await page.evaluate(() =>
      localStorage.getItem('uc.history.kit.devicesCollapsed')
    )
    await sidebar().getByRole('button', { name: 'Show Devices' }).click()
    await phone.waitFor()
    return { storedWhileCollapsed: stored }
  })

  await step('sheet-opens-from-gear', async () => {
    await openSheet()
    const local = deviceList().getByRole('option', { selected: true })
    const localName = await local.getAttribute('aria-label')
    const detail = sheet().getByRole('region', { name: 'Device details' })
    await detail.getByText('This device', { exact: true }).waitFor()
    await shot('dv2-sheet-this-device')
    return { selected: localName, sections: await deviceList().getByRole('group').count() }
  })

  await step('local-switch-and-rebuild-confirmation', async () => {
    const detail = sheet().getByRole('region', { name: 'Device details' })
    const fileSync = detail.getByRole('switch', { name: 'File sync' })
    const before = await fileSync.isChecked()
    await fileSync.click()
    const end = Date.now() + 10_000
    while ((await fileSync.isChecked()) === before && Date.now() < end)
      await page.waitForTimeout(200)
    assert.notEqual(await fileSync.isChecked(), before, 'File sync switched')
    await fileSync.click()
    await detail.getByRole('button', { name: 'Rebuild Space…' }).click()
    const alert = page.getByRole('alertdialog', { name: 'Rebuild space' })
    await alert.waitFor()
    const confirm = alert.getByRole('button', { name: 'Rebuild space' })
    const disabledBefore = await confirm.isDisabled()
    await alert.getByRole('textbox').fill('RESET')
    const enabledAfter = await confirm.isEnabled()
    await shot('dv6-rebuild-confirmation')
    await alert.getByRole('button', { name: 'Cancel' }).click()
    await alert.waitFor({ state: 'hidden' })
    assert.ok(disabledBefore && enabledAfter, 'Rebuild is confirmed only by typing RESET')
    return { fileSyncBefore: before, disabledBefore, enabledAfter }
  })

  await step('phone-detail', async () => {
    await option(PHONE).click()
    assert.equal(await option(PHONE).getAttribute('aria-selected'), 'true')
    await shot('dv5-sheet-phone')
    return { selected: await option(PHONE).getAttribute('aria-label') }
  })

  await step('invite-from-add-menu', async () => {
    await sheet().getByRole('button', { name: 'Add' }).click()
    await page.getByRole('menuitem', { name: 'Invite Device…' }).click()
    const row = deviceList().getByTestId('device-invitation-row')
    await row.waitFor({ timeout: 15_000 })
    assert.equal(
      await row.getAttribute('aria-selected'),
      'true',
      'the new invitation row is selected'
    )
    const code = sheet().getByTestId('add-device-invitation-code')
    await page.waitForFunction(
      () =>
        /^\d{3}-\d{3}$/.test(
          document.querySelector('[data-testid="add-device-invitation-code"]')?.textContent ?? ''
        ),
      null,
      { timeout: 15_000 }
    )
    const shown = (await code.innerText()).trim()
    const stored = await currentInvitation()
    assert.ok(stored, 'the daemon holds the invitation')
    assert.equal(
      shown.replace('-', ''),
      String(stored.code).replace(/\D/g, ''),
      'shown code = daemon code'
    )
    await shot('dv4-invitation-live')
    return { shown, daemonExpiresAtMs: stored.expiresAtMs }
  })

  await step('invitation-is-not-modal', async () => {
    // This device (the space was initialised as `kit-browser`).
    await option('kit-browser').click()
    const row = deviceList().getByTestId('device-invitation-row')
    assert.ok(await row.isVisible(), 'the invitation row stays while another device is selected')
    await shot('dv4-invitation-away')
    return { rowText: await row.innerText() }
  })

  await step('invitation-outlives-the-sheet', async () => {
    await closeSheet()
    const sidebarRow = sidebar().getByRole('button', { name: /^Invitation/ })
    await sidebarRow.waitFor()
    await shot('dv0-sidebar-invitation')
    const countdown = await sidebarRow.innerText()
    await sidebarRow.click()
    await deviceList().waitFor()
    const row = deviceList().getByTestId('device-invitation-row')
    assert.equal(await row.getAttribute('aria-selected'), 'true', 'the sidebar row reopens it')
    return { countdown }
  })

  await step('cancel-invitation-revokes-it', async () => {
    await sheet().getByRole('button', { name: 'Cancel Invitation' }).click()
    await deviceList().getByTestId('device-invitation-row').waitFor({ state: 'hidden' })
    const end = Date.now() + 10_000
    let stored = await currentInvitation()
    while (stored && Date.now() < end) {
      await page.waitForTimeout(300)
      stored = await currentInvitation()
    }
    assert.equal(stored, null, 'the daemon no longer holds an invitation')
    await shot('dv4-invitation-cancelled')
    return { daemonInvitation: stored }
  })

  await step('devices-route-opens-the-sheet', async () => {
    await closeSheet()
    await page.evaluate(() => {
      window.history.pushState({}, '', '/devices')
      window.dispatchEvent(new PopStateEvent('popstate'))
    })
    await deviceList().waitFor({ timeout: 15_000 })
    const pathAfter = await page.evaluate(() => location.pathname)
    assert.equal(pathAfter, '/history', '/devices redirects to History with the sheet')
    await closeSheet()
    return { pathAfter }
  })

  // With motion on, the sheet animates in and out like Manage Tags (on macOS it drops from the title
  // bar and clears the traffic lights), and it stays mounted until its exit animation has run.
  await page.context().close()
  await newPage('light', 'no-preference')
  await step('sheet-animates-in-and-out', async () => {
    const popup = () => page.locator('[role="dialog"]').filter({ has: deviceList() })
    await openSheet()
    await page.waitForTimeout(800)
    const box = await popup().boundingBox()
    // Only the macOS skin draws traffic lights; boundingBox() would wait for them elsewhere.
    const traffic = platform === 'macos' ? await page.locator('.mac-traffic').boundingBox() : null
    const viewport = page.viewportSize()
    if (platform === 'macos') {
      assert.ok(Math.abs(box.y) < 1, `sheet hangs from the top edge (y=${box.y})`)
      assert.ok(box.width <= 760, `sheet width ${box.width}`)
      assert.ok(traffic && box.x > traffic.x + traffic.width, 'sheet clears the traffic lights')
    }
    await sheet().getByRole('button', { name: 'Done', exact: true }).click()
    await page.waitForTimeout(120)
    const closing = await popup().boundingBox()
    await page.screenshot({ path: path.join(output, `${platform}-sheet-closing.png`) })
    assert.ok(closing, 'sheet is still on screen while it animates out')
    if (platform === 'macos') assert.ok(closing.y < box.y - 4, `sheet moves up (y=${closing.y})`)
    await sheet().waitFor({ state: 'detached', timeout: 5000 })
    await openSheet()
    await closeSheet()
    return { open: box, closing, traffic, viewport }
  })

  await page.context().close()
  await newPage('dark')
  await step('dark-appearance', async () => {
    await openSheet()
    await shot('dv2-sheet-dark')
    await option(PHONE).click()
    await shot('dv5-sheet-phone-dark')
    await closeSheet()
    await shot('dv0-sidebar-dark')
    return {}
  })
} finally {
  results.pageErrors = await page?.evaluate(() => window.__ucPageErrors ?? []).catch(() => null)
  await browser.close()
  server.kill()
  results.passed = results.steps.length > 0 && results.steps.every(s => s.ok)
  await writeFile(
    path.join(output, `devices-results-${platform}.json`),
    `${JSON.stringify(results, null, 2)}\n`
  )
  console.log(
    JSON.stringify({ passed: results.passed, steps: results.steps.map(s => [s.name, s.ok]) })
  )
}
assert.ok(results.passed, 'browser checks failed; see devices-results json')
