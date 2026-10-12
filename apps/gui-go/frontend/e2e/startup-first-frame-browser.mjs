// Observe the real main document before scripts, then its first React frame.
// Bootstrap is held so this browser check does not pretend to exercise a daemon.
// PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node e2e/startup-first-frame-browser.mjs --platform macos --out /tmp/startup
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { build, preview } from 'vite'

const args = process.argv.slice(2)
const option = key => args[args.indexOf(key) + 1]
const platform = option('--platform')
assert.ok(['macos', 'windows', 'linux', 'none'].includes(platform))
const out = path.resolve(option('--out'))
await mkdir(out, { recursive: true })
process.chdir(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'))
if (platform === 'none') delete process.env.UC_UI_PLATFORM
else process.env.UC_UI_PLATFORM = platform
await build({ logLevel: 'silent', build: { outDir: path.join(out, 'dist') } })
const server = await preview({
  build: { outDir: path.join(out, 'dist') },
  preview: { port: 0, host: '127.0.0.1' },
})
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const browser = await chromium.launch({ headless: true })
const results = []
try {
  for (const theme of ['light', 'dark']) {
    const page = await browser.newPage({
      viewport: { width: 1100, height: 720 },
      locale: 'zh-CN',
      reducedMotion: 'reduce',
    })
    await page.addInitScript(theme => {
      localStorage.setItem('uniclipboard.themeMode', theme)
      localStorage.setItem('uniclipboard.language', 'zh-CN')
      window.startupFrames = []
      const record = () => {
        const title = document.querySelector('#root h1')
        if (title)
          window.startupFrames.push({
            at: performance.now(),
            title: title.textContent,
            skin: document.querySelector('#root [data-ui-skin]')?.dataset.uiSkin || 'legacy',
          })
      }
      new MutationObserver(record).observe(document, { subtree: true, childList: true })
    }, theme)
    let release
    const scripts = new Promise(resolve => {
      release = resolve
    })
    await page.route('**/*.js', async route => {
      if (route.request().url().includes('/bootstrap-')) return
      await scripts
      await route.continue()
    })
    await page.goto(server.resolvedUrls.local[0], { waitUntil: 'commit' })
    await page.locator('#root h1:visible').waitFor()
    const measure = () =>
      page.evaluate(() => {
        const title = [...document.querySelectorAll('#root h1')].find(
          node => node.getBoundingClientRect().height > 0
        )
        const rect = title.getBoundingClientRect()
        const css = getComputedStyle(title)
        return {
          skin: title.closest('[data-ui-skin]')?.dataset.uiSkin || 'legacy',
          title: title.textContent,
          x: rect.x,
          y: rect.y,
          width: rect.width,
          fontSize: css.fontSize,
          textAlign: css.textAlign,
          color: css.color,
        }
      })
    const html = await measure()
    await page.screenshot({ path: path.join(out, `${theme}-html.png`) })
    release()
    await page.waitForFunction(() => !document.querySelector('.uc-splash'))
    const react = await measure()
    await page.screenshot({ path: path.join(out, `${theme}-react.png`) })
    const frames = await page.evaluate(() => window.startupFrames)
    results.push({ theme, html, react, frames })
    await page.close()
  }
  await writeFile(
    path.join(out, 'results.json'),
    JSON.stringify({ platform, kind: 'browser-main-document', results }, null, 2)
  )
  for (const row of results) {
    assert.equal(
      row.html.skin,
      platform === 'none' ? 'legacy' : platform,
      'HTML must use the selected skin before any module runs'
    )
    assert.equal(row.react.skin, row.html.skin, 'HTML to React must not switch skins')
    assert.equal(row.html.title, row.react.title)
    assert.ok(
      row.frames.every(frame => frame.skin === row.html.skin),
      'an observed frame switched skins'
    )
    if (platform !== 'none') {
      assert.equal(row.html.color, row.react.color)
      assert.equal(row.html.fontSize, row.react.fontSize)
      assert.equal(row.html.textAlign, row.react.textAlign)
      assert.ok(
        Math.abs(row.html.y - row.react.y) < 3,
        `title shifted: ${row.html.y} -> ${row.react.y}`
      )
    }
  }
  console.log(JSON.stringify({ platform, passed: true, results }))
} finally {
  await browser.close()
  await new Promise(resolve => server.httpServer.close(resolve))
}
