import assert from 'node:assert/strict'
import { mkdir } from 'node:fs/promises'

// Serve e2e/fixtures/settings-page.tsx with UC_UI_PLATFORM set to the skin under test.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.SETTINGS_TEST_URL || 'http://127.0.0.1:1455'
const output = process.env.SETTINGS_TEST_OUTPUT || '/tmp/uc-settings-sidebar'
await mkdir(output, { recursive: true })
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.PLAYWRIGHT_EXECUTABLE,
})
const errors = []
try {
  for (const theme of ['light', 'dark']) {
    const page = await browser.newPage({ viewport: { width: 560, height: 800 }, locale: 'zh-CN' })
    page.on('pageerror', error => errors.push(error.message))
    await page.goto(base)
    await page.evaluate(
      theme => document.documentElement.classList.toggle('dark', theme === 'dark'),
      theme
    )
    const show = page.getByRole('button', { name: '显示侧边栏', exact: true })
    await show.waitFor()
    assert.equal(await show.getAttribute('aria-expanded'), 'false')
    const field = page.getByRole('textbox', { includeHidden: true }).first()
    const closedBounds = await field.boundingBox()
    await page.screenshot({ path: `${output}/${theme}-narrow.png` })

    await show.click()
    const category = page.getByRole('button', { name: '外观设置', exact: true })
    await category.waitFor()
    // An overlay must not shrink or move the underlying settings column.
    const openBounds = await field.boundingBox()
    assert.ok(Math.abs(closedBounds.x - openBounds.x) < 1)
    assert.ok(Math.abs(closedBounds.width - openBounds.width) < 1)
    await page.screenshot({ path: `${output}/${theme}-drawer.png` })
    await category.click()
    await show.waitFor()
    assert.equal(await show.getAttribute('aria-expanded'), 'false')

    await show.click()
    await page.keyboard.press('Escape')
    await show.waitFor()
    assert.equal(await show.getAttribute('aria-expanded'), 'false')
    assert.equal(await show.evaluate(node => node === document.activeElement), true)

    await show.click()
    await page.setViewportSize({ width: 1280, height: 800 })
    await page.getByRole('button', { name: '通用设置', exact: true }).click()
    assert.equal(await show.count(), 0)
    await page.screenshot({ path: `${output}/${theme}-wide.png` })
    await page.setViewportSize({ width: 900, height: 800 })
    await show.waitFor()
    assert.equal(await show.getAttribute('aria-expanded'), 'false')
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
      true
    )
    await page.close()
  }
  assert.deepEqual(errors, [])
  console.log(
    'Settings sidebar: narrow/wide, category selection, Escape/focus and both themes passed'
  )
} finally {
  await browser.close()
}
