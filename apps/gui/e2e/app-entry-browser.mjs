import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'

// Visual and keyboard checks for the startup, unlock and setup screens rendered by
// e2e/fixtures/app-entry.tsx. Serve the fixture first:
//   UI_FIXTURE_ENTRY=e2e/fixtures/app-entry.tsx UI_FIXTURE_PORT=1452 node e2e/visual-effects-server.mjs
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.APP_ENTRY_TEST_URL || 'http://127.0.0.1:1452'
const output = process.env.APP_ENTRY_TEST_OUTPUT || '/tmp/uc-app-entry-verification'
await mkdir(output, { recursive: true })

const views = [
  'startup',
  'upgrade',
  'upgrade-failed',
  'membership',
  'failure',
  'version-too-old',
  'recovery',
  'unlock',
  'setup-entry',
  'setup-initialize',
  'setup-redeem',
  'setup-invite',
  'setup-pending',
  'setup-processing',
  'setup-ended',
  'setup-ready',
  'setup-paired',
  'setup-import',
]
const sizes = [
  { width: 900, height: 600 },
  { width: 1280, height: 800 },
]

const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
const results = []
const errors = []
try {
  const page = await browser.newPage({ reducedMotion: 'reduce' })
  page.on('pageerror', error => errors.push(error.message))

  async function open(view, { theme = 'light', language = 'zh-CN' } = {}) {
    await page.goto(`${base}/?${new URLSearchParams({ view, theme, language })}`)
    await page.getByTestId('entry-surface').waitFor()
    await page.evaluate(() => document.fonts.ready)
    await page.waitForTimeout(250)
  }

  async function measure() {
    return page.evaluate(() => {
      const invalidSizes = []
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
      while (walker.nextNode()) {
        const parent = walker.currentNode.parentElement
        if (!walker.currentNode.textContent.trim() || !parent) continue
        if (parent.closest('script,style,svg,[aria-hidden="true"]')) continue
        const rect = parent.getBoundingClientRect()
        if (!rect.width || !rect.height) continue
        const style = getComputedStyle(parent)
        if (!['12px', '14px', '16px', '24px'].includes(style.fontSize))
          invalidSizes.push({
            text: walker.currentNode.textContent.trim().slice(0, 40),
            size: style.fontSize,
          })
      }
      const overflowing = [...document.querySelectorAll('main *')]
        .filter(element => {
          const rect = element.getBoundingClientRect()
          // Ignore 1px visually hidden helpers such as Base UI live regions.
          return rect.width > 1 && (rect.right > innerWidth + 0.5 || rect.left < -0.5)
        })
        .map(element => element.tagName + '.' + String(element.className).slice(0, 40))
      const unnamedButtons = [...document.querySelectorAll('button')]
        .filter(button => !(button.textContent.trim() || button.getAttribute('aria-label')))
        .map(button => button.outerHTML.slice(0, 80))
      const main = document.querySelector('main')
      return {
        h1: document.querySelectorAll('h1').length,
        invalidSizes,
        overflowing,
        horizontalScroll: main ? main.scrollWidth > main.clientWidth + 1 : true,
        unnamedButtons,
      }
    })
  }

  for (const language of ['zh-CN', 'en-US']) {
    for (const theme of ['light', 'dark']) {
      for (const size of sizes) {
        await page.setViewportSize(size)
        for (const view of views) {
          await open(view, { theme, language })
          const metrics = await measure()
          const name = `${view}-${language}-${theme}-${size.width}`
          await page.screenshot({ path: path.join(output, `${name}.png`) })
          results.push({ name, ...metrics })
          assert.equal(metrics.h1, 1, `${name}: exactly one page heading`)
          assert.deepEqual(metrics.invalidSizes, [], `${name}: only the four text roles`)
          assert.deepEqual(metrics.overflowing, [], `${name}: nothing crosses the window edge`)
          assert.equal(metrics.horizontalScroll, false, `${name}: no horizontal scroll`)
          assert.deepEqual(metrics.unnamedButtons, [], `${name}: every button has a name`)
        }
      }
    }
  }

  // Unlock: keyring attempt falls back to the inline passphrase form.
  await page.setViewportSize(sizes[0])
  await open('unlock')
  await page.keyboard.press('Tab')
  assert.equal(
    await page.evaluate(() => document.activeElement?.getAttribute('data-testid')),
    'unlock-content',
    'the unlock button is the first tab stop'
  )
  await page.keyboard.press('Enter')
  await page.getByRole('button', { name: '正在解锁...' }).waitFor()
  await page.screenshot({ path: path.join(output, 'flow-unlock-busy.png') })
  const passphrase = page.locator('#unlock-passphrase')
  await passphrase.waitFor()
  assert.equal(
    await page.evaluate(() => document.activeElement?.id),
    'unlock-passphrase',
    'the passphrase field receives focus after the keyring fallback'
  )
  await passphrase.fill('wrong')
  await page.keyboard.press('Enter')
  await page.getByRole('alert').waitFor()
  assert.match(await page.getByRole('alert').innerText(), /口令错误/)
  assert.equal(await passphrase.getAttribute('aria-invalid'), 'true')
  await page.screenshot({ path: path.join(output, 'flow-unlock-wrong-passphrase.png') })
  const toggle = page.getByRole('button', { name: '显示口令' })
  await toggle.click()
  assert.equal(await passphrase.getAttribute('type'), 'text')
  assert.equal(
    await page.getByRole('button', { name: '隐藏口令' }).getAttribute('aria-pressed'),
    'true'
  )
  await passphrase.fill('fixture-passphrase')
  await page.keyboard.press('Enter')
  await page.waitForFunction(() => document.body.dataset.unlocked === 'true')

  // Reset confirmation keeps its typed guard and returns focus on Escape.
  await open('unlock')
  const resetLink = page.getByRole('button', { name: '忘记口令？重置并重新开始' })
  await resetLink.click()
  const confirm = page.getByRole('button', { name: '重置', exact: true })
  assert.equal(await confirm.isDisabled(), true)
  await page.locator('#factory-reset-confirm').fill('RESET')
  assert.equal(await confirm.isDisabled(), false)
  // Let the dialog's open transition settle before capturing it.
  await page.waitForTimeout(400)
  await page.screenshot({ path: path.join(output, 'flow-unlock-reset-dialog.png') })
  await page.keyboard.press('Escape')
  await page.locator('#factory-reset-confirm').waitFor({ state: 'detached' })
  assert.equal(
    await page.evaluate(() => document.activeElement?.textContent),
    '忘记口令？重置并重新开始',
    'focus returns to the reset link'
  )

  // Setup: keyboard navigation, field-level validation and back navigation.
  await open('setup-entry')
  await page.keyboard.press('Tab')
  assert.equal(
    await page.evaluate(() => document.activeElement?.getAttribute('data-testid')),
    'setup-entry-create'
  )
  await page.screenshot({ path: path.join(output, 'flow-setup-entry-focus.png') })
  await page.keyboard.press('Enter')
  await page.locator('#device-name').fill('Fixture Mac')
  await page.locator('#pass1').fill('one')
  await page.locator('#pass2').fill('two')
  await page.keyboard.press('Enter')
  await page.locator('#pass2-error').waitFor()
  assert.equal(await page.locator('#pass2').getAttribute('aria-invalid'), 'true')
  await page.screenshot({ path: path.join(output, 'flow-setup-initialize-mismatch.png') })
  await page.locator('#pass2').fill('one')
  await page.getByTestId('setup-initialize-submit').click()
  await page.getByText('守护进程尚未就绪，请稍后重试。').waitFor()
  await page.screenshot({ path: path.join(output, 'flow-setup-initialize-service-error.png') })
  await page.getByTestId('setup-initialize-back').click()
  await page.getByTestId('setup-entry-join').click()
  await page.keyboard.type('482913')
  assert.equal(await page.evaluate(() => document.activeElement?.id), 'join-pass')
  await page.keyboard.type('secret')
  await page.keyboard.press('Enter')
  await page.getByText('暂时无法连接对方设备，请确认两台设备都在线。').waitFor()
  await page.screenshot({ path: path.join(output, 'flow-setup-redeem-error.png') })

  await open('setup-ready')
  await page.getByTestId('setup-complete-invite').click()
  await page.getByRole('alert').waitFor()
  await page.screenshot({ path: path.join(output, 'flow-setup-invite-issue-error.png') })

  assert.deepEqual(errors, [], 'no page errors')
} finally {
  await writeFile(
    path.join(output, 'results.json'),
    JSON.stringify({ results, errors }, null, 2) + '\n'
  )
  await browser.close()
}
console.log(`App entry checks passed: ${results.length} views; evidence in ${output}`)
