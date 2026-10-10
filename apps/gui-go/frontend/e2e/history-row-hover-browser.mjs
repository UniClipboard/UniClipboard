import assert from 'node:assert/strict'
import { mkdir } from 'node:fs/promises'
import path from 'node:path'

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const base = process.env.TYPOGRAPHY_TEST_URL || 'http://127.0.0.1:1452'
const output = process.env.HISTORY_HOVER_OUTPUT || '/tmp/uc-history-hover'
await mkdir(output, { recursive: true })
const browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
try {
  const page = await browser.newPage({ reducedMotion: 'reduce' })
  for (const width of [360, 1100]) {
    await page.setViewportSize({ width, height: 800 })
    for (const theme of ['light', 'dark']) {
      await page.goto(`${base}/?view=history&theme=${theme}&language=en-US`)
      await page.getByTestId('history-row').first().waitFor()
      for (const id of ['image', 'text', 'code', 'link', 'file']) {
        const card = page.locator(`[data-entry-id="${id}"]`)
        await card.scrollIntoViewIfNeeded()
        const box = await card.evaluate(el => {
          const row = el.closest('[data-testid="history-row"]')
          const outer = row.getBoundingClientRect()
          const inner = el.getBoundingClientRect()
          return {
            outerHeight: outer.height - parseFloat(getComputedStyle(row).borderBottomWidth),
            innerHeight: inner.height,
            x: outer.x,
            bottom: outer.bottom,
            top: outer.top,
            width: outer.width,
          }
        })
        assert.ok(
          Math.abs(box.outerHeight - box.innerHeight) < 1,
          `${id}: inner ${box.innerHeight}, row ${box.outerHeight}`
        )
        // The bottom padding must belong to the same hover and click target.
        await page.mouse.move(box.x + 5, box.bottom - 3)
        const copy = card.getByRole('button', { name: 'Copy', exact: true })
        await page.waitForFunction(id => {
          const card = document.querySelector(`[data-entry-id="${id}"]`)
          return (
            getComputedStyle(card.querySelector('[data-testid="history-favorite"]').parentElement)
              .opacity === '1'
          )
        }, id)
        const geometry = await copy.evaluate(el => {
          const actions = el.parentElement.getBoundingClientRect()
          const row = el.closest('[data-testid="history-row"]').getBoundingClientRect()
          return { bottomInset: row.bottom - actions.bottom, rightInset: row.right - actions.right }
        })
        assert.ok(
          Math.abs(geometry.bottomInset - 7) < 1,
          `${id}: actions not anchored to row bottom`
        )
        assert.ok(Math.abs(geometry.rightInset - 8) < 1)
        if (id === 'image')
          await page.screenshot({ path: path.join(output, `image-hover-${theme}-${width}.png`) })
        await page.mouse.click(box.x + 5, box.bottom - 3)
        assert.ok(
          await card
            .locator('xpath=ancestor::*[@data-testid="history-row"]')
            .evaluate(el => el.className.includes('bg-primary/'))
        )
        await page.mouse.move(0, 0)
        await card.locator(':scope > button').focus()
        await page.waitForFunction(
          id =>
            getComputedStyle(
              document.querySelector(`[data-entry-id="${id}"] [data-testid="history-favorite"]`)
                .parentElement
            ).opacity === '1',
          id
        )
        assert.equal(await copy.getAttribute('tabindex'), '0')
      }
    }
  }
  console.log(
    'PASS: 20 row/type/theme/width cases; bottom-edge hover and click, action anchoring, keyboard focus'
  )
} finally {
  await browser.close()
}
