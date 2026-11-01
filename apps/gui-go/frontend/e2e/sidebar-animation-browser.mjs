// Failure modes: instant layout jump, reflowing labels, duplicate toggles during transitions, lost toggle focus, hidden focus targets, missing reverse
// transition, broken interruption, or ignoring reduced motion. Real native rendering is separate.
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import { remote } from 'webdriverio'
const width = Number(process.env.SIDEBAR_ANIMATION_WIDTH || 1100)
const theme = process.env.SIDEBAR_ANIMATION_THEME || 'light'
const fixtureUrl = new URL(process.env.SIDEBAR_ANIMATION_URL || 'http://127.0.0.1:1590/')
fixtureUrl.searchParams.set('theme', theme)
const out = process.env.SIDEBAR_ANIMATION_OUTPUT
assert.ok(out)
await mkdir(out, { recursive: true })
let browser = await remote({
  logLevel: 'error',
  capabilities: {
    browserName: 'chrome',
    'goog:chromeOptions': {
      args: ['--headless=new', `--window-size=${width},720`],
    },
  },
})
const result = {
  passed: false,
  scope: 'production macOS SplitView; synthetic browser fixture',
  width,
  theme,
  frames: {},
}
async function transition(label, reverse = false) {
  return browser.executeAsync(
    ({ label, reverse }, done) => {
      const frames = [],
        start = performance.now()
      const sidebar = document.querySelector('.mac-split-sidebar')
      const surface = sidebar.firstElementChild
      const originalToggle = document.querySelector('.mac-split-toggle')
      originalToggle.focus()
      document.querySelector(`button[aria-label="${label}"]`).click()
      if (reverse) setTimeout(() => document.querySelector('.mac-split-toggle').click(), 80)
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
          visibleToggleCount: [...document.querySelectorAll('.mac-split-toggle')].filter(button => {
            const r = button.getBoundingClientRect()
            const clip = button.closest('.mac-split-sidebar')?.getBoundingClientRect()
            return (
              r.width > 0 && r.height > 0 && (!clip || (r.right > clip.left && r.left < clip.right))
            )
          }).length,
          sameToggle: document.querySelector('.mac-split-toggle') === originalToggle,
          toggleFocused: document.activeElement === originalToggle,
          expanded: document.querySelector('.mac-split-toggle').getAttribute('aria-expanded'),
        })
        // Let the reverse action commit before starting its capture window.
        // Keep a deadline so a missing reverse action still reaches the original assertions.
        const now = performance.now()
        if (committedAt === null && (!reverse || frames.at(-1).expanded === 'true'))
          committedAt = now
        if (now - start < 2000 && (committedAt === null || now - committedAt < 400))
          requestAnimationFrame(sample)
        else done(frames)
      }
      requestAnimationFrame(sample)
    },
    { label, reverse }
  )
}
try {
  await browser.url(fixtureUrl.href)
  await browser.$('.mac-split').waitForDisplayed()
  result.actualTheme = await browser.$('[data-ui-skin="macos"]').getAttribute('data-mac-theme')
  assert.equal(result.actualTheme, theme)
  await browser.saveScreenshot(`${out}/expanded.png`)
  await browser.executeAsync(done => {
    document.querySelector('.mac-split-toggle').click()
    setTimeout(done, 55)
  })
  await browser.saveScreenshot(`${out}/collapse-middle.png`)
  await browser.executeAsync(done => setTimeout(done, 400))
  await browser.executeAsync(done => {
    document.querySelector('[aria-label="Show sidebar"]').click()
    setTimeout(done, 400)
  })
  for (const [label, key, last] of [
    ['Hide sidebar', 'collapse', 0],
    ['Show sidebar', 'expand', 236],
  ]) {
    const frames = await transition(label)
    result.frames[key] = frames
    await browser.saveScreenshot(`${out}/${key}.png`)
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
    await browser.saveScreenshot(`${out}/${key}.png`)
  }
  result.frames.reverse = await transition('Hide sidebar', true)
  assert.ok(
    result.frames.reverse.every(
      f => f.toggleCount === 1 && f.visibleToggleCount === 1 && f.sameToggle && f.toggleFocused
    )
  )
  assert.ok(Math.abs(result.frames.reverse.at(-1).width - 236) < 1)
  assert.equal(await browser.$('.mac-split-sidebar').getAttribute('inert'), null)
  await browser.$('.mac-split-toggle').click()
  await browser.keys('Space')
  await browser.waitUntil(
    async () => (await browser.$('.mac-split-toggle').getAttribute('aria-expanded')) === 'true'
  )
  await browser.executeAsync(done => setTimeout(done, 400))
  await transition('Hide sidebar')
  await browser.keys('Tab')
  assert.equal(
    await browser.execute(() =>
      document.querySelector('.mac-split-sidebar').contains(document.activeElement)
    ),
    false
  )
  assert.equal(await browser.$('.mac-split-sidebar').getAttribute('aria-hidden'), 'true')
  assert.equal(
    await browser.execute(() => {
      document.querySelector('.mac-split-sidebar button').focus()
      return document.querySelector('.mac-split-sidebar').contains(document.activeElement)
    }),
    false
  )
  await browser.deleteSession()
  browser = await remote({
    logLevel: 'error',
    capabilities: {
      browserName: 'chrome',
      'goog:chromeOptions': {
        args: ['--headless=new', `--window-size=${width},720`, '--force-prefers-reduced-motion'],
      },
    },
  })
  await browser.url(fixtureUrl.href)
  await browser.$('.mac-split').waitForDisplayed()
  assert.equal(
    await browser.execute(() => matchMedia('(prefers-reduced-motion: reduce)').matches),
    true
  )
  result.frames.reducedCollapse = await transition('Hide sidebar')
  result.frames.reduced = await transition('Show sidebar')
  assert.ok(result.frames.reduced.every(f => Math.abs(f.width - 236) < 1))
  for (const frames of [result.frames.reducedCollapse, result.frames.reduced]) {
    assert.ok(
      frames.every(
        f => f.toggleCount === 1 && f.visibleToggleCount === 1 && f.sameToggle && f.toggleFocused
      )
    )
  }
  result.passed = true
} finally {
  await writeFile(`${out}/result.json`, JSON.stringify(result, null, 2) + '\n')
  await browser.deleteSession()
}
