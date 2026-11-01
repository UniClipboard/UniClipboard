import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import { test, outputFor } from './page.mjs'
test('history tag menu original assertions', async ({ app, page }) => {
  const base = app.baseUrl
  const out = await outputFor('history-tag-menu')
  assert.ok(out, 'TAG_MENU_OUTPUT required')
  const result = {
    scope: 'production detail controls; synthetic callbacks; CSS fallback',
    steps: [],
    passed: false,
  }
  const input = () => page.locator('input[role="combobox"]')
  const shot = name => page.screenshot({ path: `${out}/${name}.png` })
  async function open() {
    await page.locator('.mac-btn').click()
    await input().waitFor()
    assert.equal(await input().evaluate(node => node === document.activeElement), true)
  }
  try {
    await page.setViewportSize({ width: 1280, height: 800 })
    for (const theme of ['light', 'dark']) {
      await page.goto(`${base}?theme=${theme}`)
      await open()
      await page.locator('[role="listbox"]').waitFor()
      await shot(`${theme}-empty`)
      await input().fill('hotfix')
      await page.locator('[data-create] [role="radiogroup"]').waitFor()
      await shot(`${theme}-create`)
      const rects = await page.evaluate(() => {
        const field = document.querySelector('.mac-combo-field').getBoundingClientRect()
        const menu = document.querySelector('.mac-combo-menu').getBoundingClientRect()
        return {
          left: menu.left - field.left,
          gap: menu.top - field.bottom,
          width: menu.width,
          right: menu.right,
          viewport: innerWidth,
          portalled: !document
            .querySelector('.mac-combo')
            .contains(document.querySelector('.mac-combo-menu')),
          colors: [...document.querySelectorAll('.mac-combo-dot')].map(
            e => getComputedStyle(e).backgroundColor
          ),
        }
      })
      assert.equal(rects.left, 0)
      assert.equal(rects.gap, 6)
      assert.equal(rects.width, 300)
      assert.ok(rects.right <= rects.viewport)
      assert.equal(rects.portalled, true)
      assert.notEqual(rects.colors[0], rects.colors[1])
      await page.locator('[role="radio"][aria-label="blue"]').click()
      assert.equal(await input().evaluate(node => node === document.activeElement), true)
      assert.equal(
        await page.locator('[role="radio"][aria-label="blue"]').getAttribute('aria-checked'),
        'true'
      )
      await page.keyboard.press('Tab')
      assert.equal(
        await page.locator('[role="radio"][aria-label="green"]').getAttribute('aria-checked'),
        'true'
      )
      await page.keyboard.press('ArrowDown')
      assert.ok(
        (await page.locator('[role="option"][aria-selected="true"]').first().innerText()).includes(
          'release'
        )
      )
      await page.keyboard.press('ArrowUp')
      assert.ok(
        (await page.locator('[role="option"][aria-selected="true"]').first().innerText()).includes(
          'hotfix'
        )
      )
      await page.keyboard.press('Enter')
      await input().waitFor({ state: 'detached' })
      assert.deepEqual(await page.evaluate(() => window.__tagMenuCreated), {
        name: 'hotfix',
        color: 'green',
      })
      await open()
      await page.keyboard.press('Escape')
      await input().waitFor({ state: 'detached' })
      await open()
      await input().fill('release')
      assert.equal((await page.locator('[data-create]').count()) > 0, false)
      await page.keyboard.press('Enter')
      await input().waitFor({ state: 'detached' })
      assert.ok((await page.locator('.mac-tag').first().innerText()).includes('release'))
      await open()
      assert.ok((await page.locator('[role="option"]').first().innerText()).includes('hot-path'))
      await page.locator('input[role="combobox"]').click()
      assert.equal((await input().count()) > 0, true)
      await page.getByRole('button', { name: 'Outside target', exact: true }).click()
      await input().waitFor({ state: 'detached' })
      result.steps.push({
        theme,
        rects,
        checks: 'empty/create/colors/Tab/arrows/Escape/Enter/exclude-attached/outside',
        passed: true,
      })
    }
    await page.goto(`${base}?empty`)
    await open()
    assert.equal((await page.locator('.mac-combo-menu').count()) > 0, false)
    await input().fill('first')
    await page.locator('.mac-combo-menu').waitFor()
    assert.equal((await page.locator('[role="option"]').all()).length, 1)
    await page.locator('.mac-root').click({ position: { x: 20, y: 20 } })
    await input().waitFor({ state: 'detached' })
    result.steps.push({ emptyCatalog: true, outsideNonFocusable: true, passed: true })
    await page.goto(base)
    await page.evaluate(() => {
      document.querySelector('section').style.width = '8rem'
    })
    await open()
    await input().fill('edge')
    await page.locator('.mac-combo-menu').waitFor()
    const edge = await page.evaluate(() => {
      const r = document.querySelector('.mac-combo-menu').getBoundingClientRect()
      return { left: r.left, right: r.right, viewport: innerWidth }
    })
    assert.ok(edge.left >= 0 && edge.right <= edge.viewport)
    await shot('edge-collision')
    result.steps.push({ edge, passed: true })
    result.passed = true
  } catch (e) {
    result.error = String(e.stack ?? e)
    await shot('failure')
    throw e
  } finally {
    await writeFile(`${out}/result.json`, JSON.stringify(result, null, 2))
  }
})
