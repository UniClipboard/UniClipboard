// Original full-app assertions translated from WebDriver to the framework-owned Playwright page.
import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, installSession } from './backend-page.mjs'
import { backendOutput as out } from './backend-suites.mjs'

test('sidebar-toggle-app: original real-backend assertions', async ({ app, page, backend }) => {
  const result = {
    passed: false,
    scope: 'complete production frontend -> real isolated daemon; native host stubbed',
    checks: [],
  }
  await installSession(page, backend)
  try {
    await page.goto(`${app.baseUrl}/history.html`)
    const toggle = page.locator('.mac-split-toggle')
    await toggle.waitFor({ timeout: 60000 })
    await page.evaluate(() =>
      [...document.querySelectorAll('button')]
        .find(b => b.textContent?.trim() === 'Disable')
        ?.click()
    )
    for (const width of [1200, 900]) {
      await page.setViewportSize({ width, height: 720 })
      await page.waitForTimeout(500)
      for (let turn = 0; turn < 4; turn++) {
        const before = await toggle.getAttribute('aria-expanded')
        await toggle.click()
        await page.waitForTimeout(400)
        const state = await page.evaluate(() => {
          const all = [...document.querySelectorAll('.mac-split-toggle')]
          const toggle = all[0]
          return {
            count: all.length,
            expanded: toggle.getAttribute('aria-expanded'),
            label: toggle.getAttribute('aria-label'),
            focused: document.activeElement === toggle,
          }
        })
        result.checks.push({ width, turn, before, ...state })
        assert.equal(state.count, 1)
        assert.notEqual(state.expanded, before)
        assert.equal(state.label, state.expanded === 'true' ? 'Hide sidebar' : 'Show sidebar')
        assert.equal(state.focused, true)
        await page.screenshot({ path: path.join(out, `${width}-${turn}.png`) })
      }
      const before = await toggle.getAttribute('aria-expanded')
      await page.keyboard.press('Control+Meta+s')
      await page.waitForTimeout(400)
      assert.notEqual(await toggle.getAttribute('aria-expanded'), before)
    }
    result.passed = true
  } finally {
    await writeFile(
      path.join(out, 'sidebar-toggle-result.json'),
      JSON.stringify(result, null, 2) + '\n'
    )
  }
})
