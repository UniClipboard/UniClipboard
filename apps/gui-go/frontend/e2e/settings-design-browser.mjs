import assert from 'node:assert/strict'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.SETTINGS_TEST_URL || 'http://127.0.0.1:1450'
const output = process.env.SETTINGS_TEST_OUTPUT || '/tmp/settings-design-verification'
const zh = JSON.parse(await readFile('src/i18n/locales/zh-CN.json', 'utf8'))
const en = JSON.parse(await readFile('src/i18n/locales/en-US.json', 'utf8'))
await mkdir(output, { recursive: true })
const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
const results = []
try {
  const context = await browser.newContext({
    viewport: { width: 1100, height: 800 },
    locale: 'zh-CN',
    reducedMotion: 'reduce',
  })
  const page = await context.newPage()
  page.setDefaultTimeout(15000)
  const errors = []
  page.on('pageerror', error => {
    errors.push(error.message)
    console.error(error.message)
  })
  await page.goto(base)
  for (const theme of ['light', 'dark']) {
    await page.emulateMedia({ colorScheme: theme })
    for (const [category, title] of Object.entries(zh.settings.categories)) {
      await page.getByRole('button', { name: title, exact: true }).click()
      await page.getByRole('heading', { level: 1, name: title, exact: true }).waitFor()
      await page.waitForFunction(() => !document.querySelector('[data-slot="skeleton"]'))
      assert.equal(await page.locator('h1').count(), 1)
      assert.equal(
        await page.locator('[data-testid="settings-page-header"] p').textContent(),
        zh.settings.pageDescriptions[category]
      )
      const geometry = await page.locator('[data-testid="settings-scroll"]').evaluate(main => {
        const heading = getComputedStyle(main.querySelector('h1'))
        const legends = [...main.querySelectorAll('[data-slot="setting-group-title"]')].map(el => ({
          tag: el.tagName,
          weight: getComputedStyle(el).fontWeight,
          size: getComputedStyle(el).fontSize,
          margin: getComputedStyle(el).marginBottom,
        }))
        const labels = [...main.querySelectorAll('[data-slot="setting-label"]')].map(
          el => getComputedStyle(el).fontWeight
        )
        return {
          overflow: main.scrollWidth > main.clientWidth,
          groups: main.querySelectorAll('[data-slot="setting-group"]').length,
          heading: { size: heading.fontSize, weight: heading.fontWeight },
          legends,
          labels,
        }
      })
      assert.equal(geometry.overflow, false, `${category}/${theme}: horizontal overflow`)
      assert.ok(geometry.groups > 0, `${category}: missing standard groups`)
      assert.deepEqual(geometry.heading, { size: '24px', weight: '600' })
      for (const legend of geometry.legends)
        assert.deepEqual(legend, { tag: 'LEGEND', weight: '600', size: '16px', margin: '16px' })
      for (const weight of geometry.labels) assert.equal(weight, '400')
      await page.screenshot({ path: path.join(output, `${category}-${theme}.png`) })
      results.push({ category, theme, ...geometry })
    }
  }
  await page.getByRole('button', { name: '通用设置', exact: true }).click()
  const name = page.getByRole('textbox', { name: '设备名称', exact: true })
  await name.fill('Visual test computer')
  await name.blur()
  assert.equal(await name.inputValue(), 'Visual test computer')
  await page.getByRole('combobox', { name: '启动方式', exact: true }).click()
  await page.getByRole('option', { name: '静默', exact: true }).click()
  await page
    .locator('[data-testid="settings-scroll"]')
    .getByText(zh.settings.sections.general.startupMode.summaries.silent, { exact: true })
    .waitFor()

  for (const [category, title] of Object.entries(zh.settings.categories)) {
    await page.setViewportSize({ width: 1100, height: 800 })
    await page.getByRole('button', { name: title, exact: true }).click()
    await page.getByRole('heading', { level: 1, name: title, exact: true }).waitFor()
    await page.setViewportSize({ width: 360, height: 800 })
    const overflow = await page
      .locator('[data-testid="settings-scroll"]')
      .evaluate(main => main.scrollWidth > main.clientWidth)
    assert.equal(overflow, false, `${category}/narrow: horizontal overflow`)
    await page.screenshot({ path: path.join(output, `${category}-narrow.png`) })
    results.push({ category, viewport: 'narrow', overflow })
  }
  const english = await browser.newPage({ viewport: { width: 900, height: 740 }, locale: 'en-US' })
  await english.goto(base)
  await english
    .getByRole('heading', { name: en.settings.categories.general, exact: true })
    .waitFor()
  await english.screenshot({ path: path.join(output, 'general-english.png') })
  assert.deepEqual(errors, [])
  await writeFile(path.join(output, 'results.json'), JSON.stringify(results, null, 2))
  console.log(
    `PASS: ${results.length} page/theme/viewport cases; device name and startup mode interactions; English header`
  )
} finally {
  await browser.close()
}
