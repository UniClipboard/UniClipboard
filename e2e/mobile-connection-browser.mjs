import assert from 'node:assert/strict'
import { mkdir, readFile } from 'node:fs/promises'

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.MOBILE_CONNECTION_TEST_URL || 'http://127.0.0.1:1467'
const output = process.env.MOBILE_CONNECTION_TEST_OUTPUT || '/tmp/uc-mobile-connection-verification'
await mkdir(output, { recursive: true })
const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
try {
  const page = await browser.newPage({ reducedMotion: 'reduce' })
  const errors = []
  page.on('pageerror', error => errors.push(error.message))
  for (const language of ['zh-CN', 'en-US', 'zh-TW', 'ja-JP', 'pt-BR', 'ru-RU']) {
    const { devices } = JSON.parse(
      await readFile(new URL(`../src/i18n/locales/${language}.json`, import.meta.url), 'utf8')
    )
    for (const theme of ['light', 'dark']) {
      for (const width of [360, 1100]) {
        await page.setViewportSize({ width, height: 720 })
        await page.goto(`${base}/?${new URLSearchParams({ language, theme })}`)
        await page.getByRole('button', { name: devices.connectMobile.title, exact: true }).click()
        const name = page.getByRole('textbox')
        await name.fill('My phone')
        const focusSpace = await name.evaluate(input => {
          const field = input.getBoundingClientRect()
          const panel = input.closest('[role="tabpanel"]').getBoundingClientRect()
          return { left: field.left - panel.left, right: panel.right - field.right }
        })
        assert.ok(
          focusSpace.left >= 3 && focusSpace.right >= 3,
          'Focus ring must fit inside the scroll viewport'
        )
        assert.equal(await page.getByRole('dialog').count(), 1)
        await page.evaluate(() => document.fonts.ready)
        for (const mode of ['regular', 'direct']) {
          if (mode === 'direct') {
            await page.getByRole('tab', { name: new RegExp(devices.connectMobile.direct) }).click()
            await page.getByTestId('add-device-invitation-code').waitFor()
          }
          const overflow = await page.getByRole('dialog').evaluate(dialog => {
            const rect = dialog.getBoundingClientRect()
            return (
              rect.left < 0 ||
              rect.right > innerWidth ||
              rect.top < 0 ||
              rect.bottom > innerHeight + 1 ||
              dialog.scrollWidth > dialog.clientWidth + 1
            )
          })
          assert.equal(overflow, false, `${language}/${theme}/${width}/${mode}: overflow`)
          const clippedButtons = await page.getByRole('tabpanel').evaluate(panel => {
            const bounds = panel.getBoundingClientRect()
            return [...panel.querySelectorAll('button')].some(button => {
              const rect = button.getBoundingClientRect()
              return (
                rect.height > 0 &&
                (rect.bottom > bounds.bottom + 1 ||
                  rect.left < bounds.left - 1 ||
                  rect.right > bounds.right + 1)
              )
            })
          })
          assert.equal(
            clippedButtons,
            false,
            `${language}/${theme}/${width}/${mode}: clipped action`
          )
          const footerLayout = await page.getByRole('dialog').evaluate(dialog => {
            const footer = dialog.querySelector('[data-slot="dialog-footer"]')
            const rect = footer.getBoundingClientRect()
            const bounds = dialog.getBoundingClientRect()
            return {
              rootChild: footer.parentElement === dialog,
              bottomGap: bounds.bottom - rect.bottom,
              leftGap: rect.left - bounds.left,
              rightGap: bounds.right - rect.right,
            }
          })
          assert.equal(footerLayout.rootChild, true, 'Footer must be outside the scrolling panels')
          assert.ok(
            footerLayout.bottomGap <= 2 && footerLayout.leftGap <= 2 && footerLayout.rightGap <= 2,
            `${language}/${theme}/${width}/${mode}: footer must span the bottom of the dialog`
          )
          await page.screenshot({ path: `${output}/${language}-${theme}-${width}-${mode}.png` })
        }
        await page.getByRole('tab', { name: devices.connectMobile.regular, exact: true }).click()
        assert.equal(await name.inputValue(), 'My phone')
        await page.getByRole('button', { name: 'Close', exact: true }).click()
        await page.getByRole('dialog').waitFor({ state: 'hidden' })
        await page.getByRole('button', { name: devices.connectMobile.title, exact: true }).click()
        assert.equal(
          await page
            .getByRole('tab', { name: devices.connectMobile.regular, exact: true })
            .getAttribute('aria-selected'),
          'true'
        )
        await page.getByRole('textbox').waitFor()
        assert.equal(await page.getByRole('textbox').inputValue(), '')
        console.log(`${language} ${theme} ${width}: passed`)
      }
    }
  }
  await page.setViewportSize({ width: 360, height: 600 })
  await page.goto(`${base}/?state=disabled`)
  await page.getByRole('button', { name: '连接手机', exact: true }).click()
  await page.getByText(/42720/).waitFor()
  const { devices } = JSON.parse(
    await readFile(new URL('../src/i18n/locales/zh-CN.json', import.meta.url), 'utf8')
  )
  await page
    .getByRole('button', { name: devices.mobileSync.enableConfirm.confirm, exact: true })
    .click()
  await page.getByRole('textbox').fill('My phone')
  await page
    .getByRole('button', { name: devices.mobileSync.add.advanced.title, exact: true })
    .click()
  const footer = page.locator('[data-slot="dialog-footer"]')
  const beforeScroll = await footer.boundingBox()
  await page.getByRole('tabpanel').evaluate(panel => {
    panel.scrollTop = panel.scrollHeight
  })
  const afterScroll = await footer.boundingBox()
  assert.deepEqual(afterScroll, beforeScroll, 'Scrolling the form must not move the footer')
  assert.ok(
    afterScroll.y + afterScroll.height <= 600,
    'Footer must remain within the short viewport'
  )
  await page.screenshot({ path: `${output}/advanced-footer.png` })
  await page.getByRole('button', { name: devices.mobileSync.add.submit, exact: true }).click()
  await page.getByRole('dialog').waitFor({ state: 'hidden' })
  assert.equal(await page.locator('body').getAttribute('data-registered'), 'true')
  assert.deepEqual(errors, [])
  console.log('Enable and register: passed; no browser errors')
} finally {
  await browser.close()
}
