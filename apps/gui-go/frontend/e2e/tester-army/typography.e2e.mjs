import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, outputFor } from './page.mjs'

test('typography: original text checks and current transfer byte label', async ({ app, page }) => {
  const base = app.baseUrl.replace(/\/$/, '')
  const output = await outputFor('typography')
  const results = []
  const errors = []
  page.on('pageerror', error => errors.push(error.message))
  const views = [
    'history',
    'devices',
    'unlock',
    'startup',
    'failure',
    'release',
    'invitation',
    'setup',
    'initialize',
    'redeem',
    'show-invitation',
    'pending',
    'rejected',
    'ready',
    'paired',
    'import',
  ]
  for (const language of ['zh-CN', 'en-US']) {
    for (const width of [360, 1100]) {
      for (const theme of ['light', 'dark']) {
        await page.setViewportSize({ width, height: 800 })
        for (const view of views) {
          await page.goto(`${base}/?${new URLSearchParams({ view, theme, language })}`)
          await page.getByTestId('typography-surface').waitFor()
          await page.evaluate(() => document.fonts.ready)
          // Wait for production entry animations to reach their final geometry.
          await page.waitForTimeout(350)
          const metrics = await page.evaluate(() => {
            const invalid = []
            const sizes = new Set()
            const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
            while (walker.nextNode()) {
              const node = walker.currentNode
              const parent = node.parentElement
              if (
                !node.textContent.trim() ||
                !parent ||
                parent.closest('script,style,svg,[aria-hidden="true"],[inert]')
              )
                continue
              const rect = parent.getBoundingClientRect()
              const style = getComputedStyle(parent)
              if (
                !rect.width ||
                !rect.height ||
                style.visibility === 'hidden' ||
                style.opacity === '0'
              )
                continue
              sizes.add(style.fontSize)
              if (
                !['12px', '14px', '16px', '24px'].includes(style.fontSize) ||
                (style.letterSpacing !== 'normal' && style.letterSpacing !== '0px')
              ) {
                invalid.push({
                  text: node.textContent.trim().slice(0, 60),
                  size: style.fontSize,
                  spacing: style.letterSpacing,
                })
              }
            }
            return {
              invalid,
              sizes: [...sizes],
              overflow: document.documentElement.scrollWidth > innerWidth,
            }
          })
          assert.deepEqual(metrics.invalid, [], `${view}/${theme}/${width}/${language}`)
          assert.equal(metrics.overflow, false, `${view}: horizontal overflow at ${width}`)
          results.push({ view, theme, width, language, ...metrics })
          if (language === 'zh-CN')
            await page.screenshot({ path: path.join(output, `${view}-${theme}-${width}.png`) })
        }
      }
    }
  }
  await page.setViewportSize({ width: 1100, height: 800 })
  await page.goto(`${base}/?view=history&language=en-US`)
  await page.locator('[data-entry-id="code"] > button').click()
  await page.getByTestId('code-preview').waitFor()
  assert.equal(
    await page.getByTestId('code-preview').evaluate(el => getComputedStyle(el).fontSize),
    '14px'
  )
  const linePositions = await page.getByTestId('code-preview').evaluate(el => {
    const number = el.querySelector('[aria-hidden] > div').getBoundingClientRect()
    const pre = el.querySelector('pre')
    return {
      numberTop: number.top,
      textTop: pre.getBoundingClientRect().top + parseFloat(getComputedStyle(pre).paddingTop),
      numberHeight: number.height,
      lineHeight: getComputedStyle(pre).lineHeight,
    }
  })
  assert.equal(linePositions.numberTop, linePositions.textTop)
  assert.equal(linePositions.numberHeight, parseFloat(linePositions.lineHeight))
  await page.locator('[data-entry-id="code"]').click({ button: 'right' })
  await page.getByRole('menu').waitFor()
  for (const item of await page.getByRole('menuitem').all()) {
    assert.equal(await item.evaluate(el => getComputedStyle(el).fontSize), '14px')
  }
  await page.screenshot({ path: path.join(output, 'history-menu.png') })
  await page.keyboard.press('Escape')
  for (const width of [360, 1100]) {
    await page.setViewportSize({ width, height: 800 })
    await page.goto(`${base}/?view=history&transfer=1`)
    const card = page.locator('[data-entry-id="file"]')
    // The production progress component presents byte counts, not a percentage label.
    await card.getByText('1.00 KB / 2.00 KB', { exact: true }).waitFor()
    const geometry = await card.evaluate(el => {
      const content = el.querySelector(':scope > .flex-1')
      const progress = [...el.querySelectorAll(':scope > div')].find(child => {
        const style = getComputedStyle(child)
        return (
          style.position === 'absolute' &&
          style.bottom !== 'auto' &&
          child.textContent.includes('/')
        )
      })
      return {
        contentBottom: content.getBoundingClientRect().bottom,
        progressTop: progress.getBoundingClientRect().top,
        fillPercent: progress.querySelector('.h-full').style.width,
      }
    })
    assert.equal(geometry.fillPercent, '50%', 'the progress fill reflects 1024 of 2048 bytes')
    assert.ok(
      geometry.contentBottom <= geometry.progressTop,
      `transfer overlaps content at ${width}`
    )
    await page.screenshot({ path: path.join(output, `transfer-${width}.png`) })
  }
  await page.goto(`${base}/?view=devices`)
  const details = page.locator('details summary').first()
  await details.click()
  assert.equal(await page.locator('details').first().getAttribute('open'), '')
  assert.deepEqual(errors, [])
  await writeFile(path.join(output, 'results.json'), JSON.stringify(results, null, 2))
  console.log(
    `PASS: ${results.length} typography cases; history selection/menu, aligned code lines, device details`
  )
})
