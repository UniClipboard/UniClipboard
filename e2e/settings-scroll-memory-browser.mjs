import assert from 'node:assert/strict'
import { mkdir } from 'node:fs/promises'

// Per-category scroll memory for the settings page (issue #1758). Serve the fixture first:
//   UI_FIXTURE_ENTRY=e2e/fixtures/settings-page.tsx UI_FIXTURE_PORT=1455 node e2e/visual-effects-server.mjs
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.SETTINGS_SCROLL_TEST_URL || 'http://127.0.0.1:1455'
const output = process.env.SETTINGS_SCROLL_TEST_OUTPUT || '/tmp/uc-settings-scroll-verification'
await mkdir(output, { recursive: true })

const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
const failures = []

async function open() {
  const page = await browser.newPage({ viewport: { width: 1100, height: 700 }, locale: 'zh-CN' })
  page.on('pageerror', error => failures.push(`page error: ${error.message}`))
  await page.goto(base)
  await page.locator('[data-slot="scroll-area-viewport"]').waitFor()
  return page
}

const viewport = page => page.locator('[data-slot="scroll-area-viewport"]')

async function selectCategory(page, name) {
  await page.getByRole('button', { name, exact: true }).click()
  await page.waitForTimeout(150)
}

const scrollTop = page => viewport(page).evaluate(node => node.scrollTop)
const maxScroll = page => viewport(page).evaluate(node => node.scrollHeight - node.clientHeight)

async function scrollTo(page, top) {
  await viewport(page).evaluate((node, value) => {
    node.scrollTop = value
  }, top)
  await page.waitForTimeout(120)
}

/** Scroll and switch category in one frame, before the scroll event is delivered. */
async function scrollAndSwitchInSameFrame(page, top, categoryName) {
  await viewport(page).evaluate(
    (node, [value, name]) => {
      node.scrollTop = value
      const button = [...document.querySelectorAll('button')].find(
        element => element.textContent.trim() === name
      )
      button.click()
    },
    [top, categoryName]
  )
  await page.waitForTimeout(150)
}

function check(label, condition, detail) {
  if (condition) {
    console.log(`ok   ${label}`)
  } else {
    console.error(`FAIL ${label}: ${detail}`)
    failures.push(`${label}: ${detail}`)
  }
}

const page = await open()
try {
  // Category names as rendered by the sidebar in zh-CN.
  const GENERAL = '通用设置'
  const SYNC = '同步设置'
  const ABOUT = '关于'

  // 1. First visit to a category starts at the top.
  check(
    'first visit starts at top',
    (await scrollTop(page)) === 0,
    `scrollTop=${await scrollTop(page)}`
  )

  // 2. Scroll general to the bottom, switch to sync: sync must start at the top.
  const generalMax = await maxScroll(page)
  assert.ok(generalMax > 0, 'general settings must be scrollable in this viewport')
  await scrollTo(page, generalMax)
  const generalSaved = await scrollTop(page)
  await selectCategory(page, SYNC)
  const syncFirst = await scrollTop(page)
  check('switching to a fresh category starts at top', syncFirst === 0, `scrollTop=${syncFirst}`)
  await page.screenshot({ path: `${output}/sync-after-switch.png` })

  // 3. Each category keeps its own position.
  const syncMax = await maxScroll(page)
  const syncTarget = Math.min(120, syncMax)
  await scrollTo(page, syncTarget)
  await selectCategory(page, GENERAL)
  const generalBack = await scrollTop(page)
  check(
    'returning to a category restores its own position',
    Math.abs(generalBack - generalSaved) <= 2,
    `expected ~${generalSaved}, got ${generalBack}`
  )
  await selectCategory(page, SYNC)
  const syncBack = await scrollTop(page)
  check(
    'the other category keeps its own position',
    Math.abs(syncBack - syncTarget) <= 2,
    `expected ~${syncTarget}, got ${syncBack}`
  )

  // 4. A short category cannot inherit an out-of-range offset.
  await selectCategory(page, GENERAL)
  await scrollTo(page, await maxScroll(page))
  await selectCategory(page, ABOUT)
  const aboutTop = await scrollTop(page)
  const aboutMax = await maxScroll(page)
  check(
    'short category stays within its own range',
    aboutTop <= aboutMax + 1,
    `scrollTop=${aboutTop}, maxScroll=${aboutMax}`
  )
  await page.screenshot({ path: `${output}/about-after-switch.png` })
  await selectCategory(page, GENERAL)
  const generalAfterShort = await scrollTop(page)
  check(
    'a short category does not shrink the stored offset of a long one',
    Math.abs(generalAfterShort - generalMax) <= 2,
    `expected ~${generalMax}, got ${generalAfterShort}`
  )

  // 5. Scrolling and switching within one frame, before the scroll event is
  //    delivered, still stores the offset the user left behind.
  const raceTarget = Math.max(0, generalMax - 60)
  await scrollAndSwitchInSameFrame(page, raceTarget, SYNC)
  await selectCategory(page, GENERAL)
  const generalAfterRace = await scrollTop(page)
  check(
    'a switch in the same frame as the scroll keeps the offset',
    Math.abs(generalAfterRace - raceTarget) <= 2,
    `expected ~${raceTarget}, got ${generalAfterRace}`
  )

  // 6. A taller window leaves a category less to scroll, so the restore is
  //    clamped. Once the window shrinks back, the stored offset must survive.
  await selectCategory(page, GENERAL)
  await scrollTo(page, generalMax)
  await selectCategory(page, SYNC)
  // General is inactive while the window grows, so only its restore is clamped.
  await page.setViewportSize({ width: 1100, height: 1400 })
  await selectCategory(page, GENERAL)
  const clampedByHeight = await scrollTop(page)
  await selectCategory(page, SYNC)
  await page.setViewportSize({ width: 1100, height: 700 })
  await selectCategory(page, GENERAL)
  const generalAfterResize = await scrollTop(page)
  check(
    'a clamped restore does not shrink the stored offset',
    Math.abs(generalAfterResize - generalMax) <= 2,
    `expected ~${generalMax}, got ${generalAfterResize} (clamped to ${clampedByHeight} while tall)`
  )

  // 7. Ordinary interaction still works after switching.
  await selectCategory(page, GENERAL)
  const deviceName = page.getByRole('textbox').first()
  await deviceName.fill('Scroll memory check')
  check(
    'settings controls still usable',
    (await deviceName.inputValue()) === 'Scroll memory check',
    await deviceName.inputValue()
  )
} finally {
  await page.close()
}

// 8. A destroyed webview (main window closed, reopened from the tray) forgets the
//    positions. A fresh page load is the browser equivalent of that webview teardown.
const reopened = await open()
try {
  check(
    'reopened window starts at top',
    (await scrollTop(reopened)) === 0,
    await scrollTop(reopened)
  )
  await reopened.screenshot({ path: `${output}/reopened.png` })
} finally {
  await reopened.close()
}

await browser.close()
if (failures.length > 0) {
  console.error(`\n${failures.length} check(s) failed`)
  process.exit(1)
}
console.log('\nall checks passed')
