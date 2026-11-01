import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import { expect } from 'e2e'
import { test, outputFor } from './page.mjs'
for (const theme of ['light', 'dark'])
  test('sidebar animation ' + theme, async ({ app, page }) => {
    const width = 1100
    const out = await outputFor('sidebar-animation-' + theme)
    assert.ok(out)
    await page.setViewportSize({ width, height: 720 })
    const fixtureUrl = new URL(app.baseUrl)
    fixtureUrl.searchParams.set('theme', theme)
    const result = {
      passed: false,
      scope: 'production macOS SplitView; synthetic browser fixture',
      width,
      theme,
      frames: {},
    }
    async function transition(label, reverse = false) {
      return page.evaluate(
        arg =>
          new Promise(done => {
            const callback = ({ label, reverse }, done) => {
              const frames = [],
                start = performance.now()
              const sidebar = document.querySelector('.mac-split-sidebar')
              const surface = sidebar.firstElementChild
              const originalToggle = document.querySelector('.mac-split-toggle')
              originalToggle.focus()
              document.querySelector(`button[aria-label="${label}"]`).click()
              let reverseClicked = false
              let committedAt = null
              function sample() {
                frames.push({
                  ms: performance.now() - start,
                  width: sidebar.getBoundingClientRect().width,
                  contentLeft:
                    document.querySelector('.mac-split-content').getBoundingClientRect().left -
                    document.querySelector('.mac-split').getBoundingClientRect().left,
                  innerWidth: surface.getBoundingClientRect().width,
                  inert: sidebar.inert,
                  toggleCount: document.querySelectorAll('.mac-split-toggle').length,
                  visibleToggleCount: [...document.querySelectorAll('.mac-split-toggle')].filter(
                    button => {
                      const r = button.getBoundingClientRect()
                      const clip = button.closest('.mac-split-sidebar')?.getBoundingClientRect()
                      return (
                        r.width > 0 &&
                        r.height > 0 &&
                        (!clip || (r.right > clip.left && r.left < clip.right))
                      )
                    }
                  ).length,
                  sameToggle: document.querySelector('.mac-split-toggle') === originalToggle,
                  toggleFocused: document.activeElement === originalToggle,
                  expanded: document
                    .querySelector('.mac-split-toggle')
                    .getAttribute('aria-expanded'),
                })
                const frame = frames.at(-1)
                if (reverse && !reverseClicked && frame.width > 1 && frame.width < 235) {
                  document.querySelector('.mac-split-toggle').click()
                  reverseClicked = true
                  frame.reverseClick = true
                }
                // Let the intermediate-width reverse action commit before sampling its final state.
                // Keep a deadline so a missing reverse action still reaches the original assertions.
                const now = performance.now()
                if (
                  committedAt === null &&
                  (!reverse || (reverseClicked && frame.expanded === 'true'))
                )
                  committedAt = now
                if (now - start < 2000 && (committedAt === null || now - committedAt < 400))
                  requestAnimationFrame(sample)
                else done(frames)
              }
              requestAnimationFrame(sample)
            }
            callback(arg, done)
          }),
        { label, reverse }
      )
    }
    try {
      await page.goto(fixtureUrl.href)
      await page.locator('.mac-split').waitFor()
      result.actualTheme = await page
        .locator('[data-ui-skin="macos"]')
        .getAttribute('data-mac-theme')
      assert.equal(result.actualTheme, theme)
      await page.screenshot({ path: `${out}/expanded.png` })
      await page.evaluate(
        () =>
          new Promise(done => {
            const callback = done => {
              document.querySelector('.mac-split-toggle').click()
              setTimeout(done, 55)
            }
            callback(done)
          })
      )
      await page.screenshot({ path: `${out}/collapse-middle.png` })
      await page.evaluate(
        () =>
          new Promise(done => {
            const callback = done => setTimeout(done, 400)
            callback(done)
          })
      )
      await page.evaluate(
        () =>
          new Promise(done => {
            const callback = done => {
              document.querySelector('[aria-label="Show sidebar"]').click()
              setTimeout(done, 400)
            }
            callback(done)
          })
      )
      for (const [label, key, last] of [
        ['Hide sidebar', 'collapse', 0],
        ['Show sidebar', 'expand', 236],
      ]) {
        const frames = await transition(label)
        result.frames[key] = frames
        await page.screenshot({ path: `${out}/${key}.png` })
        await writeFile(`${out}/result.json`, JSON.stringify(result, null, 2) + '\n')
        assert.ok(
          frames.every(f => f.toggleCount === 1),
          `${key}: exactly one toggle DOM node`
        )
        assert.ok(
          frames.every(f => f.visibleToggleCount === 1),
          `${key}: exactly one visible toggle`
        )
        assert.ok(
          frames.every(f => f.sameToggle && f.toggleFocused),
          `${key}: stable node and keyboard focus`
        )
        assert.ok(
          frames.every(f => f.expanded === String(last !== 0)),
          `${key}: aria-expanded`
        )
        assert.ok(
          frames.some(f => f.width > 1 && f.width < 235),
          `${key}: intermediate widths`
        )
        assert.ok(Math.abs(frames.at(-1).width - last) < 1)
        for (const f of frames) {
          assert.ok(Math.abs(f.innerWidth - 236) < 1)
          assert.ok(Math.abs(f.contentLeft - f.width) < 1)
        }
        await page.screenshot({ path: `${out}/${key}.png` })
      }
      result.frames.reverse = await transition('Hide sidebar', true)
      assert.ok(
        result.frames.reverse.some(f => f.reverseClick && f.width > 1 && f.width < 235),
        'reverse click must interrupt an intermediate width'
      )
      assert.ok(
        result.frames.reverse.every(
          f => f.toggleCount === 1 && f.visibleToggleCount === 1 && f.sameToggle && f.toggleFocused
        )
      )
      assert.ok(Math.abs(result.frames.reverse.at(-1).width - 236) < 1)
      assert.equal(await page.locator('.mac-split-sidebar').getAttribute('inert'), null)
      await page.locator('.mac-split-toggle').click()
      await page.keyboard.press('Space')
      await expect
        .poll(
          async () =>
            (await page.locator('.mac-split-toggle').getAttribute('aria-expanded')) === 'true'
        )
        .toBe(true)
      await page.evaluate(
        () =>
          new Promise(done => {
            const callback = done => setTimeout(done, 400)
            callback(done)
          })
      )
      await transition('Hide sidebar')
      await page.keyboard.press('Tab')
      assert.equal(
        await page.evaluate(() =>
          document.querySelector('.mac-split-sidebar').contains(document.activeElement)
        ),
        false
      )
      assert.equal(await page.locator('.mac-split-sidebar').getAttribute('aria-hidden'), 'true')
      assert.equal(
        await page.evaluate(() => {
          document.querySelector('.mac-split-sidebar button').focus()
          return document.querySelector('.mac-split-sidebar').contains(document.activeElement)
        }),
        false
      )

      await page.emulateMedia({ reducedMotion: 'reduce' })
      await page.goto(fixtureUrl.href)
      await page.locator('.mac-split').waitFor()
      assert.equal(
        await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches),
        true
      )
      result.frames.reducedCollapse = await transition('Hide sidebar')
      result.frames.reduced = await transition('Show sidebar')
      assert.ok(result.frames.reduced.every(f => Math.abs(f.width - 236) < 1))
      for (const frames of [result.frames.reducedCollapse, result.frames.reduced]) {
        assert.ok(
          frames.every(
            f =>
              f.toggleCount === 1 && f.visibleToggleCount === 1 && f.sameToggle && f.toggleFocused
          )
        )
      }
      result.passed = true
    } finally {
      await writeFile(`${out}/result.json`, JSON.stringify(result, null, 2) + '\n')
    }
  })
