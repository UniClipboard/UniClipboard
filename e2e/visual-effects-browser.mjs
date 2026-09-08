import assert from 'node:assert/strict'
import { mkdir } from 'node:fs/promises'
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.SMOOTH_TEST_URL || 'http://127.0.0.1:1439'
const output = process.env.SMOOTH_TEST_OUTPUT || '/tmp/smooth-mode-verification'
await mkdir(output, { recursive: true })
const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
try {
  const context = await browser.newContext({
    viewport: { width: 1100, height: 800 },
    reducedMotion: 'no-preference',
    locale: 'zh-CN',
  })
  context.setDefaultTimeout(12_000)
  const errors = []
  context.on('page', page =>
    page.on('pageerror', error => {
      if (!errors.includes(error.message)) {
        errors.push(error.message)
        console.error('Page error:', error.stack)
      }
    })
  )
  const main = await context.newPage()
  const panel = await context.newPage()
  await main.goto(`${base}/smooth-check`)
  await panel.goto(`${base}/smooth-check`)
  console.log('Fixture pages loaded')
  await main.getByRole('radio', { name: '效果优先', exact: true }).waitFor()
  await main.getByLabel('保留输入').fill('输入保持不变')
  await main.getByText('效果优先', { exact: true }).click()
  await panel.waitForFunction(() => document.documentElement.dataset.ucLowEffects === 'false')
  console.log('Cross-window switch applied')
  await main.getByRole('switch').click()
  await main.waitForFunction(() => {
    const style = getComputedStyle(document.querySelector('[data-testid="motion-target"]'))
    return Number(style.opacity) > 0.4 && Number(style.opacity) < 1
  })
  await main.getByText('流畅优先', { exact: true }).click()
  await main.waitForFunction(
    () =>
      Number(getComputedStyle(document.querySelector('[data-testid="motion-target"]')).opacity) ===
      0.4
  )
  assert.equal(await main.getByLabel('保留输入').inputValue(), '输入保持不变')
  await main.getByRole('button', { name: '打开弹窗' }).click()
  await main.getByRole('dialog').waitFor()
  await main.getByLabel('弹窗输入').fill('焦点检查')
  await main.keyboard.press('Escape')
  await main.getByRole('dialog').waitFor({ state: 'hidden' })
  assert.equal(
    await main
      .getByRole('button', { name: '打开弹窗' })
      .evaluate(node => node === document.activeElement),
    true
  )
  await main.screenshot({ path: `${output}/desktop-light.png` })
  await main.getByText('效果优先', { exact: true }).click()
  await main.emulateMedia({ reducedMotion: 'reduce' })
  await main.waitForFunction(
    () =>
      document.documentElement.dataset.ucReduceMotion === 'true' &&
      document.documentElement.dataset.ucLowEffects === 'false'
  )
  await main.emulateMedia({ reducedMotion: 'no-preference' })
  await main.waitForFunction(() => document.documentElement.dataset.ucReduceMotion === 'false')
  await main.getByText('自动', { exact: true }).click()
  await main.reload()
  await main.waitForFunction(() => document.documentElement.dataset.ucLowEffects === 'true')
  assert.equal(await main.getByRole('radio', { name: '自动', exact: true }).isChecked(), true)
  await main.setViewportSize({ width: 360, height: 740 })
  await main.evaluate(() => document.documentElement.classList.add('dark'))
  await main.screenshot({ path: `${output}/narrow-dark.png` })
  assert.equal(
    await main.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    true
  )
  assert.deepEqual(errors, [])
  console.log(
    'PASS: cross-window updates, active animation completion, input preservation, modal exit/focus, system preference, reload and narrow layout'
  )
} finally {
  await browser.close()
}
