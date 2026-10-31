import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, outputFor } from './page.mjs'
for (const scheme of ['light', 'dark']) {
  test(`history-compact ${scheme}`, async ({ app, page }) => {
    const base = app.baseUrl.replace(/\/$/, '')
    const output = await outputFor('history-compact')
    const results = []
    const url = base
    const errors = []
    page.on('pageerror', error => errors.push(error.message))
    await page.route('**/clipboard/**', async route => {
      const pathname = new URL(route.request().url()).pathname
      const match = pathname.match(/\/entries\/([^/]+)\/(resource|delivery)/)
      if (!match) return route.fulfill({ json: { data: [], ts: Date.now() } })
      const [, id, kind] = match
      const data =
        kind === 'delivery'
          ? { entryId: id, source: { tag: 'local' }, deliveries: [] }
          : {
              blobId: null,
              mimeType: 'text/plain',
              sizeBytes: 100,
              url: null,
              inlineData: Buffer.from(`${id}\n这是用于验证窄窗口布局的合成内容。`).toString(
                'base64'
              ),
            }
      await route.fulfill({ json: { data, ts: Date.now() } })
    })
    await page.goto(`${url}/?scheme=${scheme}`)
    await page.locator('.adw-glist [role="option"]').first().waitFor()
    const root = page.locator('.adw-split')
    const list = page.locator('[data-pane="list"]')
    const detail = page.locator('[data-pane="detail"]')
    const back = page.getByRole('button', { name: '返回列表' })
    const toggle = page.locator('.adw-split-toggle')
    async function resize(size) {
      await page.setViewportSize(size)
      await page.waitForFunction(width => {
        const split = document.querySelector('.adw-split')
        return split && split.hasAttribute('data-single-pane') === width <= 760
      }, size.width)
    }
    for (const width of [640, 739, 760, 761, 900, 1100, 1280]) {
      await resize({ width, height: 945 })
      await page.waitForTimeout(150)
      assert.equal((await root.getAttribute('data-single-pane')) !== null, width <= 760)
      const metrics = await page.evaluate(() => {
        const visible = [
          ...document.querySelectorAll('.adw-split-toolbar button, .adw-split-toolbar input'),
        ].filter(e => getComputedStyle(e).visibility !== 'hidden' && e.getClientRects().length)
        return {
          viewport: innerWidth,
          scroll: document.documentElement.scrollWidth,
          right: Math.max(...visible.map(e => e.getBoundingClientRect().right)),
        }
      })
      assert.ok(metrics.scroll <= width, JSON.stringify({ width, ...metrics }))
      assert.ok(metrics.right <= width + 1, JSON.stringify({ width, ...metrics }))
      await page.screenshot({ path: path.join(output, `${scheme}-${width}-list.png`) })
      results.push({ scheme, width, ...metrics })
    }
    await resize({ width: 739, height: 945 })
    await page.locator('.adw-glist').focus()
    await page.keyboard.press('ArrowDown')
    assert.equal(await detail.isVisible(), false, 'arrow selection must stay in the list')
    await page.keyboard.press('Enter')
    await back.waitFor()
    assert.equal(await list.isVisible(), false)
    assert.equal(
      await page.evaluate(() => document.activeElement?.classList.contains('adw-split-back')),
      true
    )
    await page.screenshot({ path: path.join(output, `${scheme}-739-detail.png`) })
    await page.keyboard.press('Escape')
    await list.waitFor({ state: 'visible' })
    assert.equal(
      await page.locator('.adw-glist').evaluate(el => el === document.activeElement),
      true
    )
    // The virtual list retains its scrolling geometry while hidden behind the detail.
    const scroller = page.locator('.adw-glist-scroller')
    await scroller.evaluate(el => {
      el.scrollTop = 700
    })
    await page.waitForTimeout(200)
    const before = await scroller.evaluate(el => el.scrollTop)
    const row = page.locator('.adw-row').filter({ hasText: '记录 12：' })
    await row.click()
    await back.waitFor()
    await back.click()
    assert.equal(await scroller.evaluate(el => el.scrollTop), before)
    // The platform selection modifier and Shift select without navigating away.
    await row.click({ modifiers: ['ControlOrMeta'] })
    assert.equal(await detail.isVisible(), false)
    await row.click({ modifiers: ['Shift'] })
    assert.equal(await detail.isVisible(), false)
    // Sidebar is modal at both compact widths, never a squeezed inline column.
    for (const width of [739, 900]) {
      await resize({ width, height: 945 })
      await toggle.click()
      const drawer = page.getByRole('dialog')
      await drawer.waitFor()
      assert.equal((await page.locator('.adw-split-content').getAttribute('inert')) !== null, true)
      await page.keyboard.press('Tab')
      assert.equal(await drawer.evaluate(el => el.contains(document.activeElement)), true)
      await page.screenshot({ path: path.join(output, `${scheme}-${width}-sidebar.png`) })
      await page.keyboard.press('Escape')
      await drawer.waitFor({ state: 'hidden' })
      assert.equal(await toggle.evaluate(el => el === document.activeElement), true)
      await toggle.click()
      await page.locator('.adw-split-sidebar').getByRole('button', { name: '置顶' }).click()
      await page.getByRole('dialog').waitFor({ state: 'hidden' })
    }
    // A detail stays open across wide/narrow transitions; returning restores one full-width list.
    await resize({ width: 739, height: 945 })
    await page.locator('.adw-glist').focus()
    await page.keyboard.press('Enter')
    await back.waitFor()
    await resize({ width: 1100, height: 945 })
    await list.waitFor({ state: 'visible' })
    assert.equal(await detail.isVisible(), true)
    await resize({ width: 640, height: 600 })
    await back.waitFor()
    await page.screenshot({ path: path.join(output, `${scheme}-640-detail.png`) })
    await back.click()
    // Search focus expands inside its own row at the minimum width.
    await page.locator('.adw-glist').focus()
    await page.keyboard.press('Enter')
    await back.waitFor()
    await page.getByRole('combobox', { name: '搜索' }).fill('合成内容')
    assert.equal(await detail.isVisible(), false)
    assert.equal(
      await page
        .getByRole('combobox', { name: '搜索' })
        .evaluate(el => el === document.activeElement),
      true
    )
    assert.ok(
      await page
        .locator('.adw-search')
        .evaluate(el => el.getBoundingClientRect().right <= innerWidth)
    )
    // The grid's inspector may unmount on return; focus must reach its remaining main pane.
    await page.locator('.adw-segment[aria-label="网格"]').click()
    await page.locator('.adw-grid').focus()
    await page.keyboard.press('Enter')
    await back.waitFor()
    await back.click()
    assert.equal(
      await page.locator('.adw-grid').evaluate(el => el === document.activeElement),
      true
    )
    assert.deepEqual(errors, [])
    await writeFile(path.join(output, `${scheme}.json`), JSON.stringify(results, null, 2))
  })
}
