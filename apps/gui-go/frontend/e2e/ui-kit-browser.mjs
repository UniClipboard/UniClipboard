// Browser check of the platform UI kit gallery (e2e/fixtures/ui-kit-gallery.tsx).
// Every interaction is located by ARIA role and name, so the same script verifies that both
// skins expose the same public semantics. Run it through the entry that serves the gallery:
//   node e2e/ui-kit-run.mjs --platform windows|macos
// UI_KIT_PLATFORM is the expected skin; the run fails when the served skin or the bundle's
// .ui-platform marker differs from it.
// A browser run is not a native WebView run: Windows WebView2 and macOS WKWebView are not covered.
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.UI_KIT_TEST_URL || 'http://127.0.0.1:1460'
const expectedPlatform = process.env.UI_KIT_PLATFORM
if (!['windows', 'macos'].includes(expectedPlatform ?? '')) {
  console.error(
    'UI_KIT_PLATFORM=windows|macos is required: run node e2e/ui-kit-run.mjs --platform <platform>'
  )
  process.exit(2)
}
const output = process.env.UI_KIT_TEST_OUTPUT || '/tmp/uc-ui-kit-verification'
await mkdir(output, { recursive: true })
const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
const results = []
const check = async (name, run) => {
  try {
    await run()
    results.push({ name, ok: true })
  } catch (error) {
    results.push({ name, ok: false, error: error.message.replace(/\s+/g, ' ').slice(0, 400) })
  }
}

try {
  const page = await browser.newPage({
    viewport: { width: 1720, height: 1200 },
    reducedMotion: 'reduce',
  })
  page.setDefaultTimeout(5000)
  const errors = []
  page.on('pageerror', error => errors.push(error.message))
  // React and the component libraries report misuse (controlled/uncontrolled switches, duplicate
  // keys, missing names) through console.error.
  page.on('console', message => {
    // The preview server has no favicon; that request is not a page error.
    if (message.type() === 'error' && !message.location().url.endsWith('/favicon.ico'))
      errors.push(`console: ${message.text()}`)
  })
  await page.goto(`${base}/?scheme=light`)
  const board = page.getByTestId('board-light')
  // The first load of a cold preview server can take longer than one interaction.
  await board.waitFor({ timeout: 30000 })
  const skin = await page.locator('[data-ui-skin]').first().getAttribute('data-ui-skin')
  await check('the served bundle is the requested platform', async () => {
    assert.equal(skin, expectedPlatform, 'data-ui-skin of the rendered UiRoot')
    const marker = await page.request.get(`${base}/.ui-platform`)
    assert.equal(marker.ok(), true, 'the gallery bundle has a .ui-platform marker')
    assert.equal((await marker.text()).trim(), expectedPlatform, '.ui-platform of the bundle')
  })
  const log = board.getByTestId('event-log')
  const expectLog = async text => assert.equal(await log.textContent(), text)
  const focusedName = () =>
    page.evaluate(() => {
      const el = document.activeElement
      return el?.getAttribute('aria-label') || el?.textContent?.trim() || el?.tagName
    })

  await check('button click reports no event object', async () => {
    await board.getByRole('button', { name: 'Copy', exact: true }).first().click()
    await expectLog('click:copy')
  })
  await check('disabled button is disabled', async () => {
    const disabled = board.getByRole('button', { name: 'Copy', exact: true }).nth(1)
    assert.equal(await disabled.isDisabled(), true)
  })
  await check('icon-only button is named by its label', async () => {
    await board.getByRole('button', { name: 'More', exact: true }).click()
    await expectLog('click:more')
  })
  await check('toggle button exposes aria-pressed', async () => {
    const pin = board.getByRole('button', { name: 'Pin', exact: true })
    assert.equal(await pin.getAttribute('aria-pressed'), 'false')
    await pin.click()
    await expectLog('pressed:true')
    assert.equal(
      await board.getByRole('button', { name: 'Pinned', exact: true }).getAttribute('aria-pressed'),
      'true'
    )
  })
  await check('search field reports the value', async () => {
    await board.getByRole('searchbox', { name: 'Search history' }).fill('docker')
    await expectLog('query:docker')
  })
  await check('switch toggles with Space', async () => {
    const sync = board.getByRole('switch', { name: 'Sync' })
    await sync.focus()
    await page.keyboard.press('Space')
    await expectLog('sync:true')
    assert.equal(await sync.isChecked(), true)
  })
  await check('checkbox reports checked state', async () => {
    await board.getByRole('checkbox', { name: 'Select', exact: true }).click()
    await expectLog('check:false')
  })
  await check('select opens a listbox in a portal and picks by keyboard', async () => {
    const device = board.getByRole('combobox', { name: 'Device' })
    await device.focus()
    await page.keyboard.press('Enter')
    const listbox = page
      .getByRole('listbox')
      .filter({ has: page.getByRole('option', { name: 'Pixel 8' }) })
    await listbox.waitFor()
    assert.equal(
      await listbox.evaluate(el => Boolean(el.closest('[data-testid="board-light"]'))),
      false
    )
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('Enter')
    await expectLog('device:pixel')
    await listbox.waitFor({ state: 'hidden' })
  })
  await check('segmented control is a named radio group; arrows move the selection', async () => {
    const group = board.getByRole('radiogroup', { name: 'Theme' })
    await group.getByRole('radio', { name: 'Dark' }).click()
    await expectLog('theme:dark')
    await page.keyboard.press('ArrowRight')
    assert.equal(await focusedName(), 'System')
    await expectLog('theme:system')
    assert.equal(await group.getByRole('radio', { name: 'System' }).isChecked(), true)
  })
  await check('menu opens by keyboard, Escape closes and returns focus', async () => {
    const trigger = board.getByRole('button', { name: 'More commands' })
    await trigger.focus()
    await page.keyboard.press('Enter')
    const menu = page.getByRole('menu')
    await menu.waitFor()
    assert.equal(await menu.getByRole('menuitem').count(), 6)
    assert.equal(
      await menu.evaluate(el => Boolean(el.closest('[data-testid="board-light"]'))),
      false
    )
    assert.equal(await focusedName().then(name => name.startsWith('Copy')), true)
    await page.keyboard.press('ArrowDown')
    assert.equal((await focusedName()).startsWith('Send to'), true)
    await page.screenshot({ path: path.join(output, `${skin}-menu-open.png`) })
    await page.keyboard.press('Escape')
    await menu.waitFor({ state: 'hidden' })
    assert.equal(await trigger.evaluate(el => el === document.activeElement), true)
  })
  await check('menu item reports its id', async () => {
    await board.getByRole('button', { name: 'More commands' }).click()
    await page.getByRole('menuitem', { name: /^Edit/ }).click()
    await expectLog('menu:edit')
  })
  await check('filter menu shows its count and reports the action', async () => {
    await board.getByRole('button', { name: /^Tag/ }).click()
    await page.getByRole('menuitem', { name: /^Invoices/ }).click()
    await expectLog('filter:invoices')
  })
  await check('context menu opens at the pointer on right-click', async () => {
    const row = board
      .getByRole('listbox', { name: 'Today' })
      .getByRole('option', { name: 'invoice-0921.pdf' })
    await row.click({ button: 'right' })
    const menu = page.getByRole('menu')
    await menu.waitFor()
    const [rowBox, menuBox] = [await row.boundingBox(), await menu.boundingBox()]
    assert.ok(Math.abs(menuBox.y - (rowBox.y + rowBox.height / 2)) < rowBox.height * 2)
    await page.getByRole('menuitem', { name: /^Pin/ }).click()
    await expectLog('context:pin')
  })
  await check('toolbar keeps one tab stop and moves with arrow keys', async () => {
    const toolbar = board.getByRole('toolbar', { name: 'Item commands' })
    await toolbar.getByRole('button', { name: 'Copy' }).focus()
    await page.keyboard.press('ArrowRight')
    assert.equal(await focusedName(), 'Send to…')
  })
  await check('item list selects, activates with Enter and deletes with Delete', async () => {
    const list = board.getByRole('listbox', { name: 'Today' })
    await list.getByRole('option', { name: 'invoice-0921.pdf' }).click()
    await expectLog('selection:invoice')
    assert.equal(
      await list.getByRole('option', { name: 'invoice-0921.pdf' }).getAttribute('aria-selected'),
      'true'
    )
    await page.keyboard.press('ArrowUp')
    assert.equal(await focusedName(), 'Q4 roadmap: ship LAN-only Mode, then per-device gates')
    await page.keyboard.press('Enter')
    await expectLog('activate:roadmap')
    await list.getByRole('option', { name: 'invoice-0921.pdf' }).focus()
    await page.keyboard.press('Delete')
    await expectLog('delete:invoice')
  })
  await check('item list jumps with Home and End', async () => {
    const list = board.getByRole('listbox', { name: 'Today' })
    await list.getByRole('option', { name: 'https://github.com/UniClipboard/UniClipboard' }).focus()
    await page.keyboard.press('End')
    assert.equal(await focusedName(), 'invoice-0921.pdf')
    await page.keyboard.press('Home')
    assert.equal(await focusedName(), 'git rebase -i HEAD~3')
  })
  // Keyboard only: how a pointer click extends the selection is platform-specific (contract).
  await check('multi-select list shows checkboxes and accumulates the selection', async () => {
    const list = board.getByRole('listbox', { name: 'Multi-select' })
    assert.equal(await list.getAttribute('aria-multiselectable'), 'true')
    assert.equal(await list.getByRole('checkbox').count(), 2)
    await list.getByRole('option', { name: 'invoice-0921.pdf' }).focus()
    await page.keyboard.press('Space')
    await expectLog('multi:roadmap,invoice')
    await page.keyboard.press('ArrowUp')
    await page.keyboard.press('Space')
    await expectLog('multi:invoice')
    await page.keyboard.press('Space')
    await expectLog('multi:invoice,roadmap')
  })
  await check('single-select list has no checkboxes', async () => {
    const list = board.getByRole('listbox', { name: 'Today' })
    assert.equal(await list.getByRole('checkbox').count(), 0)
  })
  await check('sidebar navigation moves with arrow keys and selects with Enter', async () => {
    const nav = board.getByRole('navigation', { name: 'Library' })
    await nav.getByRole('button', { name: /^All items/ }).focus()
    await page.keyboard.press('ArrowDown')
    assert.equal((await focusedName()).startsWith('Pinned'), true)
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('Enter')
    await expectLog('nav:deleted')
    // One tab stop: Tab leaves the navigation instead of walking its items.
    await page.keyboard.press('Tab')
    assert.equal(await nav.evaluate(el => el.contains(document.activeElement)), false)
  })
  await check('sidebar navigation marks the current item', async () => {
    const nav = board.getByRole('navigation', { name: 'Library' })
    await nav.getByRole('button', { name: /^Pinned/ }).click()
    await expectLog('nav:pinned')
    assert.equal(
      await nav.getByRole('button', { name: /^Pinned/ }).getAttribute('aria-current'),
      'page'
    )
  })
  await check('removable tag and search token have their own named remove button', async () => {
    // The tag text is not a remove button: clicking it must not remove the tag.
    const before = await log.textContent()
    const chip = board
      .getByRole('button', { name: 'Remove #finance', exact: true })
      .locator('xpath=..')
    await chip.getByText('#finance', { exact: true }).click()
    await expectLog(before)
    await board.getByRole('button', { name: 'Remove #finance', exact: true }).click()
    await expectLog('remove:#finance')
    await board.getByRole('button', { name: 'Remove from: iPhone 16', exact: true }).click()
    await expectLog('remove:from')
  })
  await check('notice action and dismiss', async () => {
    await board.getByRole('button', { name: 'Open settings' }).click()
    await expectLog('notice:settings')
    await board.getByRole('button', { name: 'Dismiss', exact: true }).click()
    await expectLog('notice:dismiss')
  })
  await check('alert dialog traps focus, confirms and returns focus', async () => {
    const trigger = board
      .getByRole('toolbar', { name: 'Item commands' })
      .getByRole('button', { name: 'Delete' })
    await trigger.click()
    const dialog = page.getByRole('alertdialog', { name: 'Delete from all devices?' })
    await dialog.waitFor()
    assert.equal(
      await dialog.evaluate(el => Boolean(el.closest('[data-testid="board-light"]'))),
      false
    )
    // Destructive confirmation: initial focus is on Cancel, so Enter does not destroy data.
    assert.equal(await focusedName(), 'Cancel')
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press('Tab')
      assert.equal(await dialog.evaluate(el => el.contains(document.activeElement)), true)
    }
    await page.screenshot({ path: path.join(output, `${skin}-dialog-open.png`) })
    await dialog.getByRole('button', { name: 'Delete' }).click()
    await expectLog('dialog:confirm')
    await dialog.waitFor({ state: 'hidden' })
    assert.equal(await trigger.evaluate(el => el === document.activeElement), true)
  })
  await check('dialog opened from a context menu returns focus to the row', async () => {
    const row = board
      .getByRole('listbox', { name: 'Today' })
      .getByRole('option', { name: 'git rebase -i HEAD~3' })
    await row.click({ button: 'right' })
    await page.getByRole('menuitem', { name: /^Delete from all devices/ }).click()
    const dialog = page.getByRole('alertdialog', { name: 'Delete from all devices?' })
    await dialog.waitFor()
    await page.keyboard.press('Escape')
    await dialog.waitFor({ state: 'hidden' }).catch(async () => {
      // Alert dialogs may ignore Escape; cancel explicitly.
      await dialog.getByRole('button', { name: 'Cancel' }).click()
      await dialog.waitFor({ state: 'hidden' })
    })
    assert.equal(await row.evaluate(el => el === document.activeElement), true)
  })
  await check('browser rendering never claims a native window material', async () => {
    const materials = await page
      .locator('[data-window-material]')
      .evaluateAll(els => els.map(el => el.getAttribute('data-window-material')))
    assert.ok(materials.length >= 2, 'every Surface carries data-window-material')
    for (const material of materials)
      assert.ok(['css-fallback', 'solid'].includes(material), material)
  })
  await check('Tab order follows the board', async () => {
    await page.goto(`${base}/?scheme=light`)
    await board.waitFor()
    const order = []
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press('Tab')
      order.push(await focusedName())
    }
    assert.deepEqual(order, [
      'Copy',
      'Send to…',
      'Edit',
      'More',
      'Pin',
      'Search history',
      'Type',
      'Tag1',
    ])
  })

  // Skin-specific visual tokens, from the Win11 colour board.
  if (skin === 'windows') {
    await check('windows light and dark tokens', async () => {
      await page.goto(base)
      const sample = scheme =>
        page.evaluate(scheme => {
          const root = document.querySelector(`[data-testid="board-${scheme}"]`).parentElement
          const primary = root.querySelector('button')
          return {
            base: getComputedStyle(root).backgroundColor,
            primary: getComputedStyle(primary).backgroundColor,
            primaryText: getComputedStyle(primary).color,
            weight: getComputedStyle(primary).fontWeight,
          }
        }, scheme)
      const light = await sample('light')
      const dark = await sample('dark')
      assert.deepEqual(light, {
        base: 'rgb(243, 243, 243)',
        primary: 'rgb(0, 95, 184)',
        primaryText: 'rgb(255, 255, 255)',
        weight: '400',
      })
      assert.deepEqual(dark, {
        base: 'rgb(32, 32, 32)',
        primary: 'rgb(96, 205, 255)',
        primaryText: 'rgb(0, 0, 0)',
        weight: '400',
      })
    })
    await check('portal popups inherit the scheme of their root', async () => {
      await page.goto(`${base}/?scheme=dark`)
      const dark = page.getByTestId('board-dark')
      await dark.getByRole('button', { name: 'More commands' }).click()
      const menu = page.getByRole('menu')
      await menu.waitFor()
      const color = await menu.evaluate(
        el => getComputedStyle(el.querySelector('[role="menuitem"]')).color
      )
      assert.equal(color, 'rgb(255, 255, 255)')
      // Skin variables must reach the portal mount node, not only the provider root.
      assert.equal(
        await menu.evaluate(el => getComputedStyle(el.closest('.fui-MenuPopover')).backgroundColor),
        'rgba(44, 44, 44, 0.9)'
      )
      await page.keyboard.press('Escape')
    })
  }

  for (const scheme of ['both', 'light', 'dark']) {
    await page.goto(`${base}/?scheme=${scheme}`)
    await page.getByTestId('ui-kit-gallery').waitFor()
    await page.evaluate(() => document.fonts.ready)
    await page.screenshot({
      path: path.join(output, `${skin}-gallery-${scheme}.png`),
      fullPage: true,
    })
  }
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto(`${base}/?scheme=light`)
  await check('narrow viewport has no horizontal scroll', async () => {
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
      true
    )
  })
  await page.screenshot({ path: path.join(output, `${skin}-gallery-narrow.png`), fullPage: true })
  await check('no page errors', async () => assert.deepEqual(errors, []))

  await writeFile(
    path.join(output, `${skin}-results.json`),
    JSON.stringify({ skin, base, results }, null, 2)
  )
  for (const result of results) {
    console.log(
      `${result.ok ? 'PASS' : 'FAIL'} ${result.name}${result.ok ? '' : `: ${result.error}`}`
    )
  }
  const failed = results.filter(result => !result.ok).length
  console.log(
    `${skin}: ${results.length - failed}/${results.length} passed, artifacts in ${output}`
  )
  process.exitCode = failed ? 1 : 0
} finally {
  await browser.close()
}
