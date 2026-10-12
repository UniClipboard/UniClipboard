// Failure modes: instant layout jump, reflowing labels, hidden focus targets, missing reverse
// transition, broken interruption, or ignoring reduced motion. Real native rendering is separate.
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import { remote } from 'webdriverio'
const out = process.env.SIDEBAR_ANIMATION_OUTPUT
assert.ok(out)
await mkdir(out, { recursive: true })
let browser = await remote({
  logLevel: 'error',
  capabilities: {
    browserName: 'chrome',
    'goog:chromeOptions': {
      args: ['--headless=new', '--window-size=1100,720'],
    },
  },
})
const result = {
  passed: false,
  scope: 'production macOS SplitView; synthetic browser fixture',
  frames: {},
}
async function transition(label) {
  return browser.executeAsync((label, done) => {
    const frames = [],
      start = performance.now()
    const sidebar = document.querySelector('.mac-split-sidebar')
    const surface = sidebar.firstElementChild
    document.querySelector(`button[aria-label="${label}"]`).click()
    function sample() {
      frames.push({
        ms: performance.now() - start,
        width: sidebar.getBoundingClientRect().width,
        contentLeft:
          document.querySelector('.mac-split-content').getBoundingClientRect().left -
          document.querySelector('.mac-split').getBoundingClientRect().left,
        innerWidth: surface.getBoundingClientRect().width,
        inert: sidebar.inert,
      })
      if (performance.now() - start < 400) requestAnimationFrame(sample)
      else done(frames)
    }
    requestAnimationFrame(sample)
  }, label)
}
try {
  await browser.url(process.env.SIDEBAR_ANIMATION_URL || 'http://127.0.0.1:1590/')
  await browser.$('.mac-split').waitForDisplayed()
  await browser.saveScreenshot(`${out}/expanded.png`)
  for (const [label, key, last] of [
    ['Hide sidebar', 'collapse', 0],
    ['Show sidebar', 'expand', 236],
  ]) {
    const frames = await transition(label)
    result.frames[key] = frames
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
  await browser.executeAsync(done => {
    document.querySelector('[aria-label="Hide sidebar"]').click()
    setTimeout(() => document.querySelector('[aria-label="Show sidebar"]').click(), 80)
    setTimeout(done, 450)
  })
  assert.equal(await browser.$('.mac-split-sidebar').getAttribute('inert'), null)
  await transition('Hide sidebar')
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
        args: ['--headless=new', '--window-size=1100,720', '--force-prefers-reduced-motion'],
      },
    },
  })
  await browser.url(process.env.SIDEBAR_ANIMATION_URL || 'http://127.0.0.1:1590/')
  await browser.$('.mac-split').waitForDisplayed()
  assert.equal(
    await browser.execute(() => matchMedia('(prefers-reduced-motion: reduce)').matches),
    true
  )
  await transition('Hide sidebar')
  result.frames.reduced = await transition('Show sidebar')
  assert.ok(result.frames.reduced.every(f => Math.abs(f.width - 236) < 1))
  result.passed = true
} finally {
  await writeFile(`${out}/result.json`, JSON.stringify(result, null, 2) + '\n')
  await browser.deleteSession()
}
