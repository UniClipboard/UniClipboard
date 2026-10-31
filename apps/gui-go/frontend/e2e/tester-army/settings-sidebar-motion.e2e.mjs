import assert from 'node:assert/strict'
import { test } from './page.mjs'

test('settings-sidebar-motion: original assertions', async ({ app, page }) => {
  const base = app.baseUrl.replace(/\/$/, '')
  await page.goto(base)
  await page.getByRole('button', { name: '显示侧边栏', exact: true }).waitFor()
  // The fixture defaults to reduced motion. Exercise the CSS transition explicitly as well.
  for (const reduced of [false, true]) {
    await page.evaluate(reduced => {
      document.documentElement.dataset.ucReduceMotion = String(reduced)
    }, reduced)
    for (const opening of [true, false]) {
      const frames = await page.evaluate(async () => {
        const root = document.querySelector('[data-sidebar-overlay]')
        const scrimSelector = '[class$="split-scrim"]'
        const toggle = document.querySelector('[class$="split-toggle"]')
        const initialScrim = root.querySelector(scrimSelector)
        toggle.click()
        // Effective alpha, whether the skin fades opacity or the background colour.
        const scrimAlpha = scrim => {
          const style = getComputedStyle(scrim)
          const channels = style.backgroundColor.match(/[\d.]+/g) ?? []
          const colorAlpha = channels.length > 3 ? Number(channels[3]) : 1
          return Number(style.opacity) * colorAlpha
        }
        const samples = []
        const start = performance.now()
        do {
          await new Promise(requestAnimationFrame)
          const scrim = root.querySelector(scrimSelector)
          samples.push({
            opacity: scrim ? scrimAlpha(scrim) : null,
            sameNode: scrim !== null && scrim === initialScrim,
            intercepts: scrim ? getComputedStyle(scrim).pointerEvents !== 'none' : false,
          })
        } while (performance.now() - start < 400)
        return samples
      })
      assert.ok(
        frames.every(frame => frame.sameNode),
        'keep the scrim mounted through both transitions'
      )
      if (!reduced)
        assert.ok(
          frames.some(frame => frame.opacity > 0 && frame.opacity < 1),
          'the scrim must fade instead of flashing'
        )
      const last = frames.at(-1)
      assert.equal(last.opacity > 0, opening)
      assert.equal(last.intercepts, opening)
    }
  }
  console.log('Sidebar motion: continuous scrim, fade in/out and reduced motion passed')
})
