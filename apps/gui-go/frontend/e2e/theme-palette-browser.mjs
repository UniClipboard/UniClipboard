// Serve e2e/fixtures/theme-palette.tsx with visual-effects-server.mjs, one platform at a time.
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const platform = process.env.UC_UI_PLATFORM
const output = process.env.THEME_TEST_OUTPUT || `/tmp/uc-theme-${platform}`
await mkdir(output, { recursive: true })
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH,
})
const page = await browser.newPage({
  viewport: { width: 1000, height: 800 },
  reducedMotion: 'reduce',
})
const errors = []
page.on('pageerror', error => errors.push(error.message))
const results = []
const apply = (name, mode, desktop = false, overrides = {}) =>
  page.evaluate(args => window.themeFixture.apply(...args), [name, mode, desktop, overrides])
const colors = () =>
  page.evaluate(() => {
    const paint = (el, property = 'backgroundColor') => (el ? getComputedStyle(el)[property] : null)
    const switches = Array.from(document.querySelectorAll('[role="switch"]')).map(
      el => el.closest('.fui-Switch')?.querySelector('.fui-Switch__indicator') || el
    )
    return {
      root: paint(document.querySelector('[data-ui-skin]')),
      button: paint(document.querySelector('[data-testid="primary"]')),
      buttonText: paint(document.querySelector('[data-testid="primary"]'), 'color'),
      switches: switches.map(el => paint(el)),
      selected: paint(document.querySelector('[aria-selected="true"]')),
      settingsButton: paint(
        Array.from(document.querySelectorAll('button')).find(
          el => el.textContent === 'Settings action'
        )
      ),
    }
  })
try {
  await page.goto(process.env.THEME_TEST_URL || 'http://127.0.0.1:1462')
  await page.getByTestId('primary').waitFor()
  for (const mode of ['light', 'dark']) {
    const invalid = await page.evaluate(mode => Object.entries(window.themeFixture.presets.default[mode])
      .filter(([, value]) => typeof value !== 'string' || !CSS.supports('color', value)), mode)
    assert.deepEqual(invalid, [], `${platform} ${mode}: every default token must be a valid color`)
    await apply('default', mode)
    await page.waitForTimeout(180)
    const baseline = await colors()
    for (const desktop of [false, true]) {
      await apply('supabase', mode, desktop)
      await page.waitForTimeout(250)
      const actual = await colors()
      const expected = await page.evaluate(mode => {
        const c = window.themeFixture.presets.supabase[mode]
        const context = document.createElement('canvas').getContext('2d')
        const rgb = value => {
          context.clearRect(0, 0, 1, 1)
          context.fillStyle = value
          context.fillRect(0, 0, 1, 1)
          const [r, g, b] = context.getImageData(0, 0, 1, 1).data
          return `rgb(${r}, ${g}, ${b})`
        }
        return {
          primary: rgb(c.primary),
          background: rgb(c.background),
          popover: rgb(c.popover),
          foreground: rgb(c.primaryForeground),
        }
      }, mode)
      assert.equal(actual.root, expected.background, `${platform} ${mode}: background`)
      assert.equal(actual.button, expected.primary, `${platform} ${mode}: primary button`)
      assert.equal(actual.settingsButton, expected.primary, `${platform} ${mode}: settings button`)
      assert.equal(actual.buttonText, expected.foreground, `${platform} ${mode}: button contrast`)
      assert.ok(actual.switches.length >= 2, 'both switches are present')
      for (const value of actual.switches)
        assert.equal(value, expected.primary, `${platform} ${mode}: switch`)
      assert.notEqual(actual.selected, baseline.selected, `${platform} ${mode}: selected row`)
      results.push({ mode, desktop, actual, expected })
      await page.getByRole('button', { name: 'Open menu', exact: true }).click()
      await page.getByRole('menuitem', { name: 'Menu item' }).waitFor()
      const popup = await page.getByRole('menuitem', { name: 'Menu item' }).evaluate(item => {
        let element = item.parentElement
        while (element && getComputedStyle(element).backgroundColor === 'rgba(0, 0, 0, 0)')
          element = element.parentElement
        return element && getComputedStyle(element).backgroundColor
      })
      assert.equal(popup, expected.popover, `${platform} ${mode}: popup background`)

      await page.screenshot({
        path: `${output}/${mode}-${desktop ? 'desktop' : 'preset'}.png`,
      })
      await page.keyboard.press('Escape')
    }
    await apply('default', mode)
    await page.waitForTimeout(180)
    assert.deepEqual(await colors(), baseline, `${platform} ${mode}: default restored`)
    await apply('default', mode, false, {
      primary: '#aa3377',
      background: '#252a32',
    })
    await page.waitForTimeout(180)
    const custom = await colors()
    assert.equal(custom.root, 'rgb(37, 42, 50)')
    assert.equal(custom.button, 'rgb(170, 51, 119)')
  }
  assert.deepEqual(errors, [])
  console.log(`${platform}: theme palette rendering passed`)
} finally {
  await writeFile(`${output}/results.json`, JSON.stringify({ platform, results, errors }, null, 2))
  await browser.close()
}
