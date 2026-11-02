import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, outputFor } from './page.mjs'

// Failure modes: restoring an old anchor hides the new head at the top;
// disabling restoration everywhere moves records while the user reads history.
for (const scheme of ['light', 'dark']) {
  for (const view of ['list', 'grid']) {
    test(`history top insert ${scheme} ${view}`, async ({ app, createContext }) => {
      const output = await outputFor('history-top-insert')
      const context = await createContext({
        viewport: { width: 1000, height: 700 },
        reducedMotion: 'reduce',
        recordVideo: { dir: path.join(output, 'videos'), size: { width: 1000, height: 700 } },
      })
      const page = await context.newPage()
      await page.goto(`${app.baseUrl}/?scheme=${scheme}&view=${view}`)
      const scroller = page.locator('[data-history-window-scroller]')
      await page.locator('[data-item-id="entry-0"]').waitFor()
      const settle = () =>
        page.evaluate(
          () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))
        )
      await settle()
      // Presentation pauses make the recording readable; assertions use DOM readiness.
      await page.waitForTimeout(600)
      const results = []
      for (const position of [0, 0, 0, 0, 0, 20, 400]) {
        await scroller.evaluate((element, top) => {
          element.scrollTop = top
        }, position)
        await settle()
        const before = await scroller.evaluate(element => {
          const bounds = element.getBoundingClientRect()
          const row = [...element.querySelectorAll('[data-item-id]')].find(
            row => row.getBoundingClientRect().bottom > bounds.top
          )
          return {
            id: row.dataset.itemId,
            top: row.getBoundingClientRect().top,
            scrollTop: element.scrollTop,
          }
        })
        await page.getByRole('button', { name: 'Insert entry' }).click()
        await page.locator(`[data-item-id="new-${40 + results.length}"]`).waitFor()
        await settle()
        const after = await scroller.evaluate((element, id) => {
          const bounds = element.getBoundingClientRect()
          const head = element.querySelector('[data-item-id]').getBoundingClientRect()
          const anchor = [...element.querySelectorAll('[data-item-id]')].find(
            row => row.dataset.itemId === id
          )
          return {
            scrollTop: element.scrollTop,
            headTop: head.top,
            headBottom: head.bottom,
            viewportTop: bounds.top,
            viewportBottom: bounds.bottom,
            anchorTop: anchor.getBoundingClientRect().top,
          }
        }, before.id)
        results.push({ position, before, after })
        await page.screenshot({
          path: path.join(output, `${scheme}-${view}-${results.length}-${position}.png`),
        })
        await writeFile(
          path.join(output, `${scheme}-${view}.json`),
          JSON.stringify(results, null, 2)
        )
        if (position === 0) {
          assert.equal(after.scrollTop, 0, 'new head must keep the viewport at the top')
          assert.ok(after.headTop >= after.viewportTop, 'new head is clipped above the viewport')
          assert.ok(
            after.headBottom <= after.viewportBottom,
            'new head is clipped below the viewport'
          )
        } else {
          assert.ok(
            Math.abs(after.anchorTop - before.top) <= 1,
            'insertion moved the reading position'
          )
        }
        await page.waitForTimeout(600)
      }
      const video = page.video()
      await context.close()
      await video.saveAs(path.join(output, `${scheme}-${view}.webm`))
    })
  }
}
