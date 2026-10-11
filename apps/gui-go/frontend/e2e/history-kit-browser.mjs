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
assert.ok(
  platform === 'windows' || platform === 'macos',
  'UC_UI_PLATFORM=windows|macos is required'
)
const daemon = JSON.parse(await readFile(path.join(output, 'daemon.json'), 'utf8'))
const port = Number(process.env.UI_FIXTURE_PORT || 1462)
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
    await reset().catch(() => {})
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
// After a failed step: close popups, clear the search and go back to All items, so later steps start clean.
async function reset() {
  await page.keyboard.press('Escape')
  await page.keyboard.press('Escape')
  const field = page.getByRole('searchbox', { name: 'Search history' })
  if (await field.inputValue()) await field.fill('')
  await page.getByRole('navigation').getByText('All items', { exact: true }).click()
  await rows().first().waitFor({ timeout: 10_000 })
}
async function shot(name) {
  await page.waitForTimeout(300)
  await page.screenshot({ path: path.join(output, `${name}.png`) })
}

// Daemon read-back with the driver's own unlocked session: what the page did must be persisted.
let apiToken
async function api(pathname) {
  apiToken ??= await guiSession()
  const response = await fetch(`${daemon.daemonUrl}${pathname}`, {
    headers: { Authorization: `Session ${apiToken}` },
  })
  return { status: response.status, body: response.ok ? await response.json() : null }
}
const entry = async id => api(`/clipboard/entries/${encodeURIComponent(id)}`)
// The entry list carries the pinned flag (the single-entry read does not).
async function listed(id) {
  const { body } = await api('/clipboard/entries?limit=1000')
  const items = Array.isArray(body?.data) ? body.data : []
  const found = Array.isArray(items) ? items.find(item => item.id === id) : undefined
  return found
}
// Waits until the rendered row count settles on `expected` (search and filters are debounced and async).
async function rowCountBecomes(expected, timeout = 10_000) {
  const end = Date.now() + timeout
  let count = await rows().count()
  while (count !== expected && Date.now() < end) {
    await page.waitForTimeout(200)
    count = await rows().count()
  }
  return count
}
const rows = () => page.locator('[data-item-id]')
// Ctrl+click as Windows delivers it. Chromium on macOS turns a real Ctrl+click into a context-menu click, so the
// browser run dispatches the pointer press itself; the native WebView2 run uses real input.
const ctrlClick = locator =>
  locator.evaluate(el => {
    for (const type of ['mousedown', 'mouseup', 'click'])
      el.dispatchEvent(
        new MouseEvent(type, { bubbles: true, cancelable: true, ctrlKey: true, button: 0 })
      )
  })
const row = text => rows().filter({ hasText: text }).first()
const list = () => page.getByRole('listbox', { name: 'Clipboard history' })
const detail = () => page.getByRole('region', { name: 'Item detail' })
const selectedIds = () =>
  page.evaluate(() =>
    [...document.querySelectorAll('[data-item-id][aria-selected="true"]')].map(
      el => el.dataset.itemId
    )
  )
const activeDescription = () =>
  page.evaluate(() => {
    const el = document.activeElement
    return el
      ? `${el.tagName.toLowerCase()}[role=${el.getAttribute('role')}][name=${el.getAttribute('aria-label') ?? el.textContent?.trim().slice(0, 40)}]`
      : null
  })
async function newPage(options) {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    reducedMotion: 'reduce',
    colorScheme: 'light',
    ...options,
  })
  page = await context.newPage()
  page.on('pageerror', error => results.consoleErrors.push(error.message))
  await page.goto(await pageUrl())
  await rows().first().waitFor({ timeout: 60_000 })
  await dismissStartupPrompts()
}
results.consoleErrors = []
const geometry = () =>
  page.evaluate(() => {
    const box = selector => {
      const el = document.querySelector(selector)
      if (!el) return null
      const r = el.getBoundingClientRect()
      return {
        x: Math.round(r.x),
        y: Math.round(r.y),
        w: Math.round(r.width),
        h: Math.round(r.height),
      }
    }
    return {
      searchField: box('[data-search-field]'),
      navPane: box('[data-surface="sidebar"]'),
      listPane: box('[data-pane="list"]'),
      detailPane: box('[data-pane="detail"]'),
      row: box('[data-item-id]'),
      pageOverflowX: document.documentElement.scrollWidth - window.innerWidth,
      skin: document.querySelector('[data-ui-skin]')?.getAttribute('data-ui-skin'),
      colorScheme: document.querySelector('[data-ui-skin]')?.getAttribute('data-color-scheme'),
      material: [...document.querySelectorAll('[data-window-material]')].map(el =>
        el.getAttribute('data-window-material')
      ),
    }
  })

async function seededRun() {
  await newPage()
  await step('page-loads', async () => {
    await shot('default-light')
    const g = await geometry()
    assert.equal(g.skin, 'windows')
    assert.ok(
      g.material.every(m => m === 'solid'),
      `window material ${g.material}`
    )
    assert.ok(g.pageOverflowX <= 0, `horizontal overflow ${g.pageOverflowX}`)
    return { rows: await rows().count(), geometry: g }
  })

  await step('select-and-detail', async () => {
    await row('git rebase -i HEAD~3').click()
    await detail().getByText('git rebase -i HEAD~3').first().waitFor()
    const ids = await selectedIds()
    assert.equal(ids.length, 1)
    return { selected: ids }
  })

  await step('keyboard-arrows', async () => {
    const before = (await selectedIds())[0]
    await page.keyboard.press('ArrowDown')
    const down = (await selectedIds())[0]
    assert.notEqual(down, before, 'ArrowDown moved the selection')
    await page.keyboard.press('ArrowUp')
    assert.equal((await selectedIds())[0], before, 'ArrowUp moved it back')
    await page.keyboard.press('Home')
    const first = await rows().first().getAttribute('data-item-id')
    assert.equal((await selectedIds())[0], first, 'Home selects the first row')
    return { focus: await activeDescription() }
  })

  await step('copy', async () => {
    await row('https://github.com/UniClipboard/UniClipboard').click()
    const restore = page.waitForResponse(r => r.url().includes('/clipboard/restore/'))
    await detail().getByRole('button', { name: 'Copy' }).click()
    const response = await restore
    assert.ok(response.ok(), `restore ${response.status()}`)
    await detail().getByRole('button', { name: 'Copied' }).waitFor({ timeout: 5000 })
    await shot('copy-feedback')
    return { restore: response.status() }
  })

  await step('pin-persists', async () => {
    const target = row('Q4 roadmap')
    await target.click()
    const id = await target.getAttribute('data-item-id')
    await detail().getByRole('button', { name: 'Pin' }).click()
    await detail().getByRole('button', { name: 'Unpin' }).waitFor()
    const read = await listed(id)
    const favorited = read?.isFavorited
    assert.equal(favorited, true, `daemon read-back ${JSON.stringify(read)}`)
    await page.getByRole('navigation').getByText('Pinned', { exact: true }).click()
    await row('Q4 roadmap').waitFor()
    const pinnedRows = await rowCountBecomes(1)
    assert.equal(pinnedRows, 1, 'Pinned view shows the pinned entry only')
    await shot('pinned-view')
    await page.getByRole('navigation').getByText('All items', { exact: true }).click()
    await row('Paging entry').waitFor()
    return { id, favorited, pinnedRows }
  })

  await step('search-and-delete-key-in-field', async () => {
    await row('git rebase').click()
    await page.keyboard.press('Control+f')
    const field = page.getByRole('searchbox', { name: 'Search history' })
    assert.ok(await field.evaluate(el => el === document.activeElement), 'Ctrl+F focuses search')
    await field.fill('docker')
    await row('docker compose').waitFor()
    const matches = await rowCountBecomes(1)
    assert.equal(matches, 1, `search "docker" matches ${matches}`)
    await shot('search-results')
    await page.keyboard.press('End')
    await page.keyboard.press('Delete')
    await page.keyboard.press('Backspace')
    await page.waitForTimeout(300)
    assert.equal(
      await page.getByRole('alertdialog').count(),
      0,
      'Delete in the field opens no dialog'
    )
    assert.equal(await field.inputValue(), 'docke', 'Backspace edited the text only')
    await field.fill('no-such-entry-zz')
    const empty = await rowCountBecomes(0)
    await shot('search-no-results')
    await field.fill('')
    await row('Paging entry').waitFor()
    return { matches, noResultRows: empty }
  })

  await step('filter-type', async () => {
    await page.getByRole('combobox', { name: 'Type' }).click()
    await page.getByRole('option', { name: 'Link' }).click()
    await row('https://github.com').waitFor()
    const links = await rowCountBecomes(1)
    assert.equal(links, 1, `Type=Link shows ${links}`)
    await shot('filter-link')
    await page.getByRole('combobox', { name: 'Type' }).click()
    await page.getByRole('option', { name: 'Any Type' }).click()
    await row('Paging entry').waitFor()
    return { links }
  })

  await step('multi-select', async () => {
    await rows().nth(0).click()
    await rows()
      .nth(2)
      .click({ modifiers: ['Shift'] })
    let ids = await selectedIds()
    assert.equal(ids.length, 3, `shift-click range ${ids.length}`)
    await detail().getByText('3 Items Selected').waitFor()
    await ctrlClick(rows().nth(1))
    ids = await selectedIds()
    assert.equal(ids.length, 2, `ctrl-click removes ${ids.length}`)
    const checkboxes = await page
      .locator('[data-item-id] input[type="checkbox"], [data-item-id] [role="checkbox"]')
      .count()
    await shot('multi-select')
    await page.keyboard.press('Control+a')
    await page.waitForTimeout(300)
    const all = (await selectedIds()).length
    assert.ok(all >= 10, `select all ${all}`)
    await page.keyboard.press('Escape')
    return { range: 3, afterCtrlClick: 2, selectAll: all, checkboxes }
  })

  await step('context-menu-focus-return', async () => {
    const target = row('git rebase')
    await target.click()
    await target.click({ button: 'right' })
    const menu = page.getByRole('menu')
    await menu.waitFor()
    await shot('context-menu')
    const items = await menu.getByRole('menuitem').allTextContents()
    await page.keyboard.press('Escape')
    await menu.waitFor({ state: 'hidden' })
    const focus = await activeDescription()
    assert.ok(/listbox/.test(focus ?? ''), `focus after menu: ${focus}`)
    return { items, focus }
  })

  await step('delete-confirm-cancel-restores-focus', async () => {
    const target = row('Meeting notes')
    await target.click()
    await page.keyboard.press('Delete')
    const dialog = page.getByRole('alertdialog')
    await dialog.waitFor()
    await shot('delete-confirm')
    const focusInDialog = await activeDescription()
    await page.keyboard.press('Escape')
    await dialog.waitFor({ state: 'hidden' })
    const focus = await activeDescription()
    assert.ok(/listbox/.test(focus ?? ''), `focus after cancel: ${focus}`)
    assert.equal(await row('Meeting notes').count(), 1)
    return { focusInDialog, focus }
  })

  await step('delete-one-persists', async () => {
    const target = row('Meeting notes')
    const id = await target.getAttribute('data-item-id')
    await target.click()
    await page.keyboard.press('Delete')
    await page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click()
    await target.waitFor({ state: 'detached' })
    const read = await entry(id)
    assert.notEqual(read.status, 200, `deleted entry still readable: ${read.status}`)
    return { id, readBack: read.status }
  })

  await step('delete-batch-persists', async () => {
    const a = row('Paging entry 129')
    const b = row('Paging entry 128')
    const ids = [await a.getAttribute('data-item-id'), await b.getAttribute('data-item-id')]
    await a.click()
    await ctrlClick(b)
    await detail().getByText('2 Items Selected').waitFor()
    await page.keyboard.press('Delete')
    await page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click()
    await page.waitForTimeout(1500)
    const reads = await Promise.all(ids.map(entry))
    results.debugBatch = {
      reads: reads.map(r => r.status),
      rendered: [await a.count(), await b.count()],
    }
    await a.waitFor({ state: 'detached' })
    await b.waitFor({ state: 'detached' })
    assert.ok(
      reads.every(r => r.status !== 200),
      `read-back ${reads.map(r => r.status)}`
    )
    return { ids, readBack: reads.map(r => r.status) }
  })

  await step('paging', async () => {
    const before = await rows().count()
    const loaded = page.locator('[data-item-id]').filter({ hasText: 'Paging entry 000' })
    for (let i = 0; i < 40 && !(await loaded.count()); i++) {
      await list().focus()
      await page.keyboard.press('End')
      await page.waitForTimeout(300)
    }
    await loaded.first().waitFor({ timeout: 10_000 })
    return { renderedBefore: before, reachedOldest: true }
  })

  await step('dark', async () => {
    await page.context().close()
    await newPage({ colorScheme: 'dark' })
    await row('git rebase').click()
    await shot('default-dark')
    const g = await geometry()
    assert.equal(g.colorScheme, 'dark')
    await row('git rebase').click({ button: 'right' })
    await page.getByRole('menu').waitFor()
    await shot('context-menu-dark')
    await page.keyboard.press('Escape')
    return { colorScheme: g.colorScheme }
  })

  await step('narrow-and-scaled', async () => {
    const sizes = [
      { name: 'narrow-1000', viewport: { width: 1000, height: 800 }, deviceScaleFactor: 2 },
      { name: 'narrow-760', viewport: { width: 760, height: 640 }, deviceScaleFactor: 2 },
      { name: 'scaled-150', viewport: { width: 853, height: 533 }, deviceScaleFactor: 1.5 },
    ]
    const out = []
    for (const size of sizes) {
      await page.context().close()
      await newPage({ viewport: size.viewport, deviceScaleFactor: size.deviceScaleFactor })
      await shot(size.name)
      const g = await geometry()
      assert.ok(g.pageOverflowX <= 0, `${size.name} horizontal overflow ${g.pageOverflowX}`)
      out.push({ name: size.name, geometry: g })
    }
    return out
  })
}

// First launch: an initialised space with no history (board WA1), light and dark.
async function emptyRun() {
  for (const colorScheme of ['light', 'dark']) {
    await step(`empty-${colorScheme}`, async () => {
      if (page) await page.context().close()
      const context = await browser.newContext({
        viewport: { width: 1280, height: 800 },
        deviceScaleFactor: 2,
        reducedMotion: 'reduce',
        colorScheme,
      })
      page = await context.newPage()
      await page.goto(await pageUrl())
      // The first-run consent alert is modal (it hides the page from the accessibility tree), so wait on text.
      await page.getByText('No Selection').waitFor({ timeout: 60_000 })
      await dismissStartupPrompts()
      await page.waitForTimeout(1500)
      assert.equal(await rows().count(), 0, 'no rows')
      await shot(`empty-${colorScheme}`)
      return {
        geometry: await geometry(),
        detailText: (await detail().innerText()).slice(0, 200),
        listPaneText: (await page.locator('[data-pane="list"]').innerText()).slice(0, 200),
        listboxRendered: await list().count(),
      }
    })
  }
}

try {
  if (daemon.seeded === 0) await emptyRun()
  else await seededRun()
} finally {
  results.pageErrors = await page?.evaluate(() => window.__ucPageErrors ?? []).catch(() => null)
  results.nativeCalls = await page?.evaluate(() => window.__ucNativeCalls ?? []).catch(() => null)
  await browser.close()
  server.kill()
  results.passed = results.steps.every(s => s.ok)
  await writeFile(
    path.join(output, 'browser-results.json'),
    `${JSON.stringify(results, null, 2)}\n`
  )
  console.log(
    JSON.stringify({ passed: results.passed, steps: results.steps.map(s => [s.name, s.ok]) })
  )
}
assert.ok(results.passed, 'browser checks failed; see browser-results.json')
