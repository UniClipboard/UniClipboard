// Production detail controls in an isolated browser. Failure modes: clipped popup, wrong anchor,
// missing tag colors, blur before pointer selection, wrong keyboard selection, stale attached tags,
// unreadable themes, and no dismissal. This is not daemon persistence or WKWebView acceptance.
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import { remote } from 'webdriverio'
const base = process.env.TAG_MENU_URL || 'http://127.0.0.1:1583/'
const out = process.env.TAG_MENU_OUTPUT
assert.ok(out, 'TAG_MENU_OUTPUT required')
await mkdir(out, { recursive: true })
const browser = await remote({
  logLevel: 'error',
  capabilities: {
    browserName: 'chrome',
    'goog:chromeOptions': { args: ['--headless=new', '--window-size=1280,800'] },
  },
})
const result = {
  scope: 'production detail controls; synthetic callbacks; CSS fallback',
  steps: [],
  passed: false,
}
const input = () => browser.$('input[role="combobox"]')
const shot = name => browser.saveScreenshot(`${out}/${name}.png`)
async function open() {
  await browser.$('.mac-btn').click()
  await input().waitForDisplayed()
  assert.equal(await input().isFocused(), true)
}
try {
  await browser.setViewport({ width: 1280, height: 800 })
  for (const theme of ['light', 'dark']) {
    await browser.url(`${base}?theme=${theme}`)
    await open()
    await browser.$('[role="listbox"]').waitForDisplayed()
    await shot(`${theme}-empty`)
    await input().setValue('hotfix')
    await browser.$('[data-create] [role="radiogroup"]').waitForDisplayed()
    await shot(`${theme}-create`)
    const rects = await browser.execute(() => {
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
    await browser.$('[role="radio"][aria-label="blue"]').click()
    assert.equal(await input().isFocused(), true)
    assert.equal(
      await browser.$('[role="radio"][aria-label="blue"]').getAttribute('aria-checked'),
      'true'
    )
    await browser.keys('Tab')
    assert.equal(
      await browser.$('[role="radio"][aria-label="green"]').getAttribute('aria-checked'),
      'true'
    )
    await browser.keys('ArrowDown')
    assert.ok(
      (await browser.$('[role="option"][aria-selected="true"]').getText()).includes('release')
    )
    await browser.keys('ArrowUp')
    assert.ok(
      (await browser.$('[role="option"][aria-selected="true"]').getText()).includes('hotfix')
    )
    await browser.keys('Enter')
    await input().waitForExist({ reverse: true })
    assert.deepEqual(await browser.execute(() => window.__tagMenuCreated), {
      name: 'hotfix',
      color: 'green',
    })
    await open()
    await browser.keys('Escape')
    await input().waitForExist({ reverse: true })
    await open()
    await input().setValue('release')
    assert.equal(await browser.$('[data-create]').isExisting(), false)
    await browser.keys('Enter')
    await input().waitForExist({ reverse: true })
    assert.ok((await browser.$('.mac-tag').getText()).includes('release'))
    await open()
    assert.ok((await browser.$('[role="option"]').getText()).includes('hot-path'))
    await browser.$('input[role="combobox"]').click()
    assert.equal(await input().isExisting(), true)
    await browser.$('button=Outside target').click()
    await input().waitForExist({ reverse: true })
    result.steps.push({
      theme,
      rects,
      checks: 'empty/create/colors/Tab/arrows/Escape/Enter/exclude-attached/outside',
      passed: true,
    })
  }
  await browser.url(`${base}?empty`)
  await open()
  assert.equal(await browser.$('.mac-combo-menu').isExisting(), false)
  await input().setValue('first')
  await browser.$('.mac-combo-menu').waitForDisplayed()
  assert.equal((await browser.$$('[role="option"]')).length, 1)
  await browser.$('.mac-root').click({ x: 20, y: 20 })
  await input().waitForExist({ reverse: true })
  result.steps.push({ emptyCatalog: true, outsideNonFocusable: true, passed: true })
  await browser.url(base)
  await browser.execute(() => {
    document.querySelector('section').style.width = '8rem'
  })
  await open()
  await input().setValue('edge')
  await browser.$('.mac-combo-menu').waitForDisplayed()
  const edge = await browser.execute(() => {
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
  await browser.deleteSession()
}
