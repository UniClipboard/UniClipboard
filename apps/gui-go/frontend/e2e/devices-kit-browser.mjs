import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { spawn } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { openGuiSession } from './browser-daemon-session.mjs'
import { credentialReopen } from './devices-kit-lifecycle.mjs'
import { passphraseLifecycle } from './devices-kit-passphrase-lifecycle.mjs'
import { redactEvidence } from './evidence-redaction.mjs'

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

export async function runScenarios({
  daemon,
  output,
  platform,
  createContext,
  preparePage,
  currentInvitation,
}) {
  const PHONE = 'kit-image'
  const results = {
    head: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
    dirty: execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim() !== '',
    platform,
    mode: 'browser-fixture',
    profile: daemon.profile,
    steps: [],
    consoleErrors: [],
  }
  let page
  async function step(name, run, { sensitive = false } = {}) {
    try {
      const detail = await run()
      results.steps.push({ name, ok: true, detail })
      console.log(`PASS ${name}`)
    } catch (error) {
      results.steps.push({ name, ok: false, error: redactEvidence(String(error?.stack ?? error)) })
      console.log(`FAIL ${name}`)
      await writeFile(
        path.join(output, `diagnostic-${name}.json`),
        redactEvidence(
          JSON.stringify(
            await page.evaluate(() => ({
              path: location.pathname,
              dialogs: [...document.querySelectorAll('[role="dialog"]')].map(el => ({
                label: el.getAttribute('aria-label'),
                hidden: el.getAttribute('aria-hidden'),
                height: el.getBoundingClientRect().height,
              })),
              options: [...document.querySelectorAll('[role="option"]')].map(el => ({
                label: el.getAttribute('aria-label'),
                selected: el.getAttribute('aria-selected'),
              })),
              buttons: [...document.querySelectorAll('button')]
                .map(el => el.getAttribute('aria-label') || el.textContent?.trim())
                .filter(Boolean),
            })),
            null,
            2
          )
        )
      )
      if (!sensitive)
        await page
          ?.screenshot({
            path: path.join(output, `failed-${name}.png`),
            mask: [
              page.getByTestId('add-device-invitation-code'),
              page.locator('svg[aria-label*="QR"]'),
            ],
          })
          .catch(() => {})
    }
  }
  async function shot(name) {
    await page.waitForTimeout(400)
    await page.screenshot({
      path: path.join(output, `${platform}-${name}.png`),
      mask: [page.getByTestId('add-device-invitation-code'), page.locator('svg[aria-label*="QR"]')],
    })
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
    const context = await createContext({
      viewport: { width: 1280, height: 800 },
      deviceScaleFactor: 2,
      reducedMotion,
      colorScheme,
    })
    page = await context.newPage()
    page.on('pageerror', error => results.consoleErrors.push(error.message))
    await preparePage(page)
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
      const manage = sidebar().getByRole('button', { name: 'Manage Devices…' })
      assert.equal(
        (await manage.innerText()).trim(),
        'Manage Devices…',
        'entry has a visible label'
      )
      await openSheet()
      await closeSheet()
      await sidebar().getByRole('button', { name: 'Show Devices' }).click()
      await phone.waitFor()
      return { storedWhileCollapsed: stored }
    })

    await step('sheet-opens-from-labelled-entry', async () => {
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
      return { shownCodeMatchesDaemon: true, daemonExpiresAtMs: stored.expiresAtMs }
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

    // With motion on, the Sheet stays mounted until its exit animation finishes.
    // Native macOS traffic lights are outside this full-app fixture's DOM.
    await page.context().close()
    await newPage('light', 'no-preference')
    await step('sheet-animates-in-and-out', async () => {
      const popup = () =>
        page
          .locator('[role="dialog"]')
          .filter({ has: page.getByRole('listbox', { name: 'Devices', includeHidden: true }) })
      await openSheet()
      await page.waitForTimeout(800)
      const box = await popup().boundingBox()
      const viewport = page.viewportSize()
      assert.ok(box && box.width > 0 && box.height > 0, 'the Sheet has visible geometry')
      assert.ok(
        box.x >= 0 && box.x + box.width <= viewport.width + 1,
        'the Sheet fits the viewport'
      )
      if (platform === 'macos') {
        assert.ok(Math.abs(box.y) < 1, `sheet hangs from the top edge (y=${box.y})`)
        assert.ok(box.width <= 760, `sheet width ${box.width}`)
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
      return {
        open: box,
        closing,
        viewport,
        nativeTrafficLightClearance:
          platform === 'macos' ? 'requires-native-acceptance' : 'not-applicable',
      }
    })

    if (process.env.DEVICES_KIT_LIFECYCLE === '1') {
      await openSheet()
      for (const branch of ['password', 'username']) {
        await step(
          `credential-${branch}-close-reopen`,
          () => credentialReopen({ page, sheet, option, openSheet, closeSheet }, branch),
          { sensitive: true }
        )
      }
      await closeSheet()
    }

    if (process.env.DEVICES_KIT_PASSPHRASE_LIFECYCLE === '1') {
      for (const outcome of ['success', 'failure']) {
        for (const detachOwner of [false, true]) {
          await page.context().close()
          await newPage('light')
          await step(
            `passphrase-${outcome}-${detachOwner ? 'detached' : 'retained'}`,
            () =>
              passphraseLifecycle(
                { page, sheet, option, openSheet, closeSheet },
                outcome,
                detachOwner
              ),
            { sensitive: true }
          )
        }
      }
    }

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
    results.passed = results.steps.length > 0 && results.steps.every(s => s.ok)
    await writeFile(
      path.join(output, `devices-results-${platform}.json`),
      `${redactEvidence(JSON.stringify(results, null, 2))}\n`
    )
    console.log(
      JSON.stringify({ passed: results.passed, steps: results.steps.map(s => [s.name, s.ok]) })
    )
  }
  assert.ok(results.passed, 'browser checks failed; see devices-results json')
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await runLegacy()
