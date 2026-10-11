// Browser E2E for the macOS design prototype (src/prototypes/macos-design). It drives the real Base UI components
// with mouse and keyboard and saves state screenshots. Start the page first:
//   cd apps/gui-go/frontend && UC_DEV_SERVER_PORT=1531 npx vite
//   PLAYWRIGHT_MODULE=<playwright entry> node e2e/macos-design-browser.mjs
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.MACOS_DESIGN_URL || 'http://127.0.0.1:1531/macos-design-preview.html'
const output = process.env.MACOS_DESIGN_OUTPUT || '/tmp/macos-design-verification'
await mkdir(output, { recursive: true })

const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
const findings = {}
try {
  const context = await browser.newContext({
    viewport: { width: 1180, height: 820 },
    deviceScaleFactor: 2,
    reducedMotion: 'no-preference',
  })
  context.setDefaultTimeout(8_000)
  const errors = []
  context.on('page', page => page.on('pageerror', error => errors.push(error.message)))
  const page = await context.newPage()
  const shot = async (name, target = page) => target.screenshot({ path: `${output}/${name}.png` })

  await page.goto(`${base}?theme=light`)
  const list = page.getByRole('listbox', { name: 'All items' })
  await list.waitFor()
  await shot('window-light')

  // Selection: click, arrow keys, shift-extend, ⌘-click.
  await page.getByRole('option', { name: /git rebase/ }).click()
  await page.keyboard.press('ArrowDown')
  assert.equal(
    await page.getByRole('option', { name: /docker compose logs/ }).getAttribute('aria-selected'),
    'true'
  )
  await page.keyboard.press('Shift+ArrowDown')
  assert.equal(await list.locator('[aria-selected="true"]').count(), 2)
  await page.getByRole('option', { name: /invoice-0921/ }).click({ modifiers: ['Meta'] })
  assert.equal(await list.locator('[aria-selected="true"]').count(), 3)
  await shot('list-multi-select')
  await page.getByRole('option', { name: /compose\.prod/ }).click()
  console.log('List selection: click, arrows, shift and command extend')

  // Enter is the default action (Copy) and raises the in-window notification substitute.
  await page.keyboard.press('Enter')
  await page.locator('.mac-toast').getByText('Copied').waitFor()
  await shot('toast-copied')
  console.log('Enter triggers Copy')

  // ⌘F focuses the toolbar search field; typing filters the list.
  await page.keyboard.press('Meta+f')
  const search = page.getByRole('searchbox', { name: 'Search' })
  assert.equal(await search.evaluate(node => node === document.activeElement), true)
  await search.fill('docker')
  assert.equal(await list.getByRole('option').count(), 2)
  await shot('search-focused')
  await search.fill('')
  console.log('Command-F focuses search and filters')

  // Right-click menu: opens at the pointer, arrow keys highlight, Escape closes.
  await page.getByRole('option', { name: /github\.com/ }).click({ button: 'right' })
  const menu = page.getByRole('menu')
  await menu.waitFor()
  await page.keyboard.press('ArrowDown')
  await page.waitForTimeout(250)
  await shot('context-menu-open')
  findings.contextMenuInsideViewport = await menu.evaluate(node => {
    const rect = node.getBoundingClientRect()
    return rect.right <= window.innerWidth && rect.bottom <= window.innerHeight
  })
  await page.keyboard.press('Escape')
  await menu.waitFor({ state: 'hidden' })
  // Right-click on an unselected row selects it first, as Finder does.
  assert.equal(
    await page.getByRole('option', { name: /github\.com/ }).getAttribute('aria-selected'),
    'true'
  )
  console.log('Context menu opens, navigates and closes')

  // Toolbar "More" menu grows from its trigger (transform-origin from the positioner).
  const more = page.getByRole('button', { name: 'More' })
  await more.click()
  await menu.waitFor()
  findings.menuTransformOrigin = await menu.evaluate(node => getComputedStyle(node).transformOrigin)
  await page.waitForTimeout(250)
  await shot('more-menu-open')
  await page.keyboard.press('Escape')
  await menu.waitFor({ state: 'hidden' })
  assert.equal(
    await more.evaluate(node => node === document.activeElement),
    true,
    'focus returns to the trigger'
  )
  console.log('More menu returns focus to its trigger')

  // Toolbar is one tab stop with arrow-key movement between its buttons.
  await page.keyboard.press('ArrowLeft')
  findings.toolbarArrowMovesTo = await page.evaluate(() =>
    document.activeElement?.getAttribute('aria-label')
  )
  assert.equal(findings.toolbarArrowMovesTo, 'Pin')

  // Help tag appears after hovering, like an AppKit tooltip.
  await page.getByRole('button', { name: 'Pin' }).hover()
  await page.locator('.mac-tooltip').getByText('Pin (⌘P)').waitFor()
  await shot('tooltip')
  await page.mouse.move(10, 10)
  console.log('Tooltip shows label and shortcut')

  // Send to… popover with checkboxes; the primary button counts the targets.
  await page.getByRole('button', { name: 'Send to…' }).first().click()
  const popover = page.getByRole('dialog', { name: 'Send to' })
  await popover.waitFor()
  await page.waitForTimeout(250)
  await shot('popover-send-to')
  await popover.getByRole('checkbox').first().click()
  await popover.getByRole('button', { name: 'Send to 1' }).click()
  await popover.waitFor({ state: 'hidden' })
  await page.locator('.mac-toast').getByText('Arrived on 1 devices.').waitFor()
  console.log('Send popover toggles targets and sends')

  // Delete key opens the alert; Escape cancels and returns focus to the list; Delete confirms.
  await page.getByRole('option', { name: /invoice-0921/ }).click()
  await page.keyboard.press('Backspace')
  const alert = page.getByRole('alertdialog', { name: 'Delete from all devices?' })
  await alert.waitFor()
  await page.waitForTimeout(300)
  await shot('alert-delete')
  await page.keyboard.press('Escape')
  await alert.waitFor({ state: 'hidden' })
  findings.focusAfterAlertCancel = await page.evaluate(() =>
    document.activeElement?.getAttribute('aria-label')
  )
  await page.keyboard.press('Backspace')
  await alert.waitFor()
  await alert.getByRole('button', { name: 'Delete' }).click()
  await alert.waitFor({ state: 'hidden' })
  assert.equal(await page.getByRole('option', { name: /invoice-0921/ }).count(), 0)
  console.log('Delete alert cancels and confirms')

  // T opens the tag sheet that drops from the title bar.
  await page.getByRole('option', { name: /compose\.prod/ }).click()
  await page.keyboard.press('t')
  const sheet = page.getByRole('dialog', { name: 'Add Tag' })
  await sheet.waitFor()
  await page.waitForTimeout(450)
  await shot('sheet-add-tag')
  await sheet.getByRole('textbox', { name: 'Tag name' }).fill('urgent')
  await page.keyboard.press('Enter')
  await sheet.waitFor({ state: 'hidden' })
  await page.locator('.mac-detail').getByText('#urgent').waitFor()
  console.log('Tag sheet adds a tag')

  // Window losing focus greys the selection (web approximation of the key window).
  // Headless Chromium keeps document.hasFocus() true, so the blur is simulated.
  await page.evaluate(() => {
    document.hasFocus = () => false
    window.dispatchEvent(new Event('blur'))
  })
  findings.inactiveSelection = await page
    .locator('.mac-row[aria-selected="true"]')
    .first()
    .evaluate(node => getComputedStyle(node).backgroundColor)
  assert.equal(findings.inactiveSelection, 'rgb(220, 220, 220)')
  await shot('window-inactive')

  // Tab order: the web reaches every control; macOS without Full Keyboard Access only reaches lists and text fields.
  await page.reload()
  await list.waitFor()
  await page.locator('body').click({ position: { x: 5, y: 400 } })
  const stops = []
  for (let i = 0; i < 12; i++) {
    await page.keyboard.press('Tab')
    stops.push(
      await page.evaluate(() => {
        const el = document.activeElement
        return el?.getAttribute('aria-label') || el?.textContent?.trim().slice(0, 20) || el?.tagName
      })
    )
  }
  findings.webTabStops = stops
  await page.keyboard.press('Shift+Tab')
  await shot('focus-ring-keyboard')

  // Dark theme: popups portal into the themed root and keep dark tokens.
  await page.goto(`${base}?theme=dark&generation=27`)
  await list.waitFor()
  await shot('window-dark-27')
  await page.getByRole('option', { name: /git rebase/ }).click({ button: 'right' })
  await menu.waitFor()
  await page.waitForTimeout(250)
  findings.darkMenuGlass = await menu.evaluate(node => getComputedStyle(node).backgroundColor)
  assert.match(findings.darkMenuGlass, /rgba\(40, 40, 44/)
  await shot('context-menu-dark')
  await page.keyboard.press('Escape')
  await menu.waitFor({ state: 'hidden' })

  // Reduced motion: popups fade without scale or blur.
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await more.click()
  await menu.waitFor()
  findings.reducedMotionMenuTransition = await menu.evaluate(
    node => getComputedStyle(node).transitionProperty
  )
  await page.keyboard.press('Escape')
  await page.emulateMedia({ reducedMotion: 'no-preference' })

  // C1 controls with real state changes: pop-up, segmented, switch, checkbox, disabled, focus-visible.
  const board = await context.newPage()
  await board.setViewportSize({ width: 1720, height: 820 })
  await board.goto(`${base}?board=c1`)
  const light = board.getByRole('region', { name: 'Light' })
  const popUp = light.getByRole('combobox', { name: 'Sort order' })
  await popUp.click()
  const listbox = board.getByRole('listbox')
  await listbox.waitFor()
  findings.popUpOverlaysTrigger = await board.evaluate(
    () => document.querySelector('.mac-menu')?.getAttribute('data-side') === 'none'
  )
  await board.waitForTimeout(200)
  await shot('popup-open', board)
  await board.keyboard.press('ArrowDown')
  await board.keyboard.press('Enter')
  await listbox.waitFor({ state: 'hidden' })
  assert.equal((await popUp.textContent())?.includes('Oldest first'), true)
  // Segmented control: a radio group, so arrow keys move the selection like NSSegmentedControl.
  await light.getByRole('radio', { name: 'Grid' }).click()
  assert.equal(
    await light.getByRole('radio', { name: 'Grid' }).getAttribute('aria-checked'),
    'true'
  )
  await board.keyboard.press('ArrowLeft')
  assert.equal(
    await light.getByRole('radio', { name: 'List' }).getAttribute('aria-checked'),
    'true'
  )
  const sync = light.getByRole('switch', { name: 'Sync images' })
  await sync.focus()
  await board.keyboard.press('Space')
  assert.equal(await sync.getAttribute('aria-checked'), 'true')
  const box = light.getByRole('checkbox', { name: 'Include pinned' })
  await box.click()
  assert.equal(await box.getAttribute('aria-checked'), 'true')
  const disabled = light.locator('.mac-btn[data-disabled]')
  assert.equal(await disabled.count(), 1)
  findings.disabledButtonIsNativeDisabled = await disabled.evaluate(node =>
    node.hasAttribute('disabled')
  )
  assert.equal(findings.disabledButtonIsNativeDisabled, true)
  await light.getByRole('button', { name: 'Cancel' }).focus()
  await board.keyboard.press('Tab')
  await board.keyboard.press('Shift+Tab')
  await shot('c1-interacted', board)
  const pressTarget = light.getByRole('button', { name: 'Cancel' })
  const pressBox = await pressTarget.boundingBox()
  await board.mouse.move(pressBox.x + pressBox.width / 2, pressBox.y + pressBox.height / 2)
  await board.mouse.down()
  await board.waitForTimeout(120)
  findings.pressedTransform = await pressTarget.evaluate(node => getComputedStyle(node).transform)
  await board.mouse.up()
  console.log('C1 controls change state')

  assert.deepEqual(errors, [])
  await writeFile(`${output}/findings.json`, JSON.stringify(findings, null, 2))
  console.log(JSON.stringify(findings, null, 2))
} finally {
  await browser.close()
}
