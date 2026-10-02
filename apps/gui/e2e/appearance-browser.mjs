import assert from 'node:assert/strict'
import { mkdir } from 'node:fs/promises'
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
const output = process.env.APPEARANCE_TEST_OUTPUT || '/tmp/appearance-verification'
await mkdir(output, { recursive: true })
try {
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 }, locale: 'zh-CN' })
  const errors = []
  page.on('pageerror', error => {
    errors.push(error.message)
    console.error(error.message)
  })
  await page.goto(process.env.APPEARANCE_TEST_URL || 'http://127.0.0.1:1441')
  await page.getByText('浅色', { exact: true }).waitFor()
  const controls = await page.locator('.appearance-row').evaluateAll(rows =>
    rows
      .map(row => {
        const control = row.querySelector(':scope > .grid, :scope > [role="combobox"]')
        if (!control) return null
        const rect = control.getBoundingClientRect()
        return { left: rect.left, width: rect.width }
      })
      .filter(Boolean)
  )
  assert.equal(controls.length, 4)
  for (const control of controls) {
    assert.ok(Math.abs(control.left + control.width - controls[0].left - controls[0].width) < 1)
  }
  assert.equal(await page.locator('h3').count(), 0)
  assert.equal(await page.locator('[data-appearance-theme-preview]').count(), 3)
  assert.equal(await page.getByRole('radio', { name: '跟随系统', exact: true }).isChecked(), true)
  assert.equal(await page.getByRole('heading', { name: '外观设置', exact: true }).count(), 1)
  assert.equal(await page.locator('details').getAttribute('open'), null)
  await page.screenshot({ path: `${output}/light.png`, fullPage: true })
  await page.getByRole('radio', { name: '跟随系统', exact: true }).focus()
  await page.keyboard.press('ArrowRight')
  await page.waitForFunction(
    () => document.querySelector('input[name="appearance-theme"][value="light"]').checked
  )
  await page.getByText('深色', { exact: true }).click()
  await page.waitForFunction(() => document.documentElement.classList.contains('dark'))
  await page.getByRole('combobox', { name: '深色配色', exact: true }).click()
  await page.getByRole('option', { name: 'blue', exact: true }).click()
  assert.match(
    await page.getByRole('combobox', { name: '深色配色', exact: true }).textContent(),
    /blue/i
  )
  await page.getByRole('combobox', { name: '深色配色', exact: true }).click()
  await page.getByRole('option', { name: 'rose', exact: true }).click()
  await page.waitForFunction(() => {
    const probe = document.createElement('span')
    probe.style.color = 'var(--primary)'
    document.body.append(probe)
    const primary = getComputedStyle(probe).color
    probe.remove()
    const preview = document.querySelector('[data-appearance-theme-preview="dark"]')
    const radio = document.querySelector('.appearance-theme-radio[data-selected="true"]')
    return (
      getComputedStyle(preview).outlineColor === primary &&
      getComputedStyle(radio).borderColor === primary &&
      getComputedStyle(radio, '::after').backgroundColor === primary
    )
  })
  await page.getByText('自定义颜色', { exact: true }).click()
  await page.getByRole('button', { name: '背景色', exact: true }).last().click()
  const hex = page.getByRole('textbox', { name: '背景色: 十六进制颜色值', exact: true })
  await hex.fill('#17201f')
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '背景色: 恢复为预设', exact: true }).last().click()
  await page.getByText('自定义颜色', { exact: true }).click()
  await page.getByText('效果优先', { exact: true }).click()
  await page.waitForFunction(() => document.documentElement.dataset.ucLowEffects === 'false')
  await page.getByRole('button', { name: '已采用你的选择。', exact: true }).hover()
  await page.getByRole('tooltip').waitFor()
  await page.mouse.move(1, 1)
  await page.getByRole('tooltip').waitFor({ state: 'hidden' })
  await page.getByRole('button', { name: '放大界面', exact: true }).click()
  assert.match(await page.getByRole('combobox', { name: '界面缩放' }).textContent(), /110%/)
  await page.getByRole('button', { name: '重置为 100%', exact: true }).click()
  assert.match(await page.getByRole('combobox', { name: '界面缩放' }).textContent(), /100%/)
  await page.screenshot({ path: `${output}/dark.png`, fullPage: true })
  await page.setViewportSize({ width: 360, height: 800 })
  await page.screenshot({ path: `${output}/narrow.png`, fullPage: true })
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true)
  assert.deepEqual(errors, [])
  console.log(
    'PASS: reference theme previews, palette, custom color/reset, effects, zoom/reset and narrow layout'
  )
} finally {
  await browser.close()
}
