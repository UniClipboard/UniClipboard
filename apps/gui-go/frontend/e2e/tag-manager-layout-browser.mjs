import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'

// Run from frontend after history_kit_browser_daemon.py start --empty --out <dir>.
// UC_UI_PLATFORM=macos node e2e/tag-manager-layout-browser.mjs <dir>
// Full frontend and real isolated daemon; the fixture stubs only the native host.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
assert.equal(process.env.UC_UI_PLATFORM, 'macos')
const output = path.resolve(process.argv[2])
await mkdir(output, { recursive: true })
const daemon = JSON.parse(await readFile(path.join(output, 'daemon.json'), 'utf8'))
const conn = JSON.parse(await readFile(daemon.conn, 'utf8'))
const post = async (pathname, body, authorization) => {
  const response = await fetch(`${daemon.daemonUrl}${pathname}`, {
    method: 'POST',
    headers: { Authorization: authorization, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  assert.ok(response.ok, `${pathname}: ${response.status}`)
  return (await response.json()).data
}
const session = await post(
  '/auth/connect',
  { pid: process.pid, clientType: 'gui' },
  `Bearer ${conn.token}`
)
const token = session.sessionToken
await post('/content-lock/unlock', { passphrase: daemon.passphrase }, `Session ${token}`)
await post('/clipboard/dispatch', { text: 'Tag manager layout fixture' }, `Session ${token}`)
const settingsResponse = await fetch(`${daemon.daemonUrl}/settings`, {
  method: 'PUT',
  headers: { Authorization: `Session ${token}`, 'Content-Type': 'application/json' },
  body: JSON.stringify({ general: { language: 'zh-CN' } }),
})
assert.ok(settingsResponse.ok, `settings: ${settingsResponse.status}`)
const tagsResponse = await fetch(`${daemon.daemonUrl}/history/tags`, {
  headers: { Authorization: `Session ${token}` },
})
assert.ok(tagsResponse.ok)
const tags = (await tagsResponse.json()).data
for (let n = 0; n < 7; n++) {
  if (!tags.some(tag => tag.name === `Layout ${n}`)) {
    await post('/history/tags', { name: `Layout ${n}` }, `Session ${token}`)
  }
}
const port = Number(process.env.UI_FIXTURE_PORT || 1463)
const server = spawn('node', ['e2e/visual-effects-server.mjs'], {
  env: {
    ...process.env,
    UI_FIXTURE_ENTRY: 'e2e/fixtures/history-full-app.tsx',
    UI_FIXTURE_PORT: String(port),
  },
  stdio: ['ignore', 'pipe', 'inherit'],
})
let browser
const results = []
try {
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('fixture server timeout')), 180_000)
    server.stdout.on('data', chunk => {
      if (String(chunk).includes(`127.0.0.1:${port}`)) {
        clearTimeout(timer)
        resolve()
      }
    })
    server.on('exit', code => {
      clearTimeout(timer)
      reject(new Error(`fixture exited ${code}`))
    })
  })
  browser = await chromium.launch({ headless: true, channel: process.env.PLAYWRIGHT_CHANNEL })
  for (const colorScheme of ['light', 'dark']) {
    for (const width of [1280, 760, 640]) {
      const page = await browser.newPage({ viewport: { width, height: 800 }, colorScheme })
      await page.addInitScript(() => localStorage.setItem('uniclipboard.language', 'zh-CN'))
      await page.goto(
        `http://127.0.0.1:${port}/?${new URLSearchParams({ daemon: daemon.daemonUrl, token })}`
      )
      const consent = page.getByRole('button', { name: '关闭', exact: true })
      const more = page.getByRole('button', { name: '更多', exact: true }).first()
      await consent.or(more).first().waitFor()
      if (await consent.isVisible()) await consent.click()
      await more.click()
      await page.getByRole('menuitem', { name: '管理标签', exact: true }).click()
      const header = page.locator('.mac-sheet-header')
      await header.waitFor()
      await page.waitForTimeout(350)
      const geometry = await header.evaluate(element => {
        const measure = node => {
          const rect = node.getBoundingClientRect()
          const range = document.createRange()
          range.selectNodeContents(node)
          const lines = [...range.getClientRects()].filter(r => r.width > 0 && r.height > 0)
          return {
            text: node.textContent,
            width: rect.width,
            height: rect.height,
            lineTops: [...new Set(lines.map(r => Math.round(r.top)))],
          }
        }
        return {
          count: measure(element.querySelector('.mac-text')),
          sort: measure(element.querySelector('.mac-popup-btn').firstElementChild),
          search: measure(element.querySelector('.mac-search')),
          overflow: element.scrollWidth > element.clientWidth,
        }
      })
      await page.screenshot({ path: path.join(output, `tags-${colorScheme}-${width}.png`) })
      results.push({ colorScheme, width, geometry })
      assert.equal(geometry.count.lineTops.length, 1, 'tag count wraps')
      assert.equal(geometry.sort.lineTops.length, 1, 'sort label wraps')
      assert.ok(geometry.search.width >= 100, 'search remains usable')
      assert.equal(geometry.overflow, false, 'header overflows')
      const search = page.getByRole('searchbox', { name: '筛选标签' })
      await search.fill('Layout 3')
      await page.getByRole('grid').getByText('#Layout 3', { exact: true }).waitFor()
      assert.equal(await page.getByRole('grid').getByRole('row').count(), 2)
      await page.close()
    }
  }
} catch (error) {
  const page = browser?.contexts().at(-1)?.pages().at(-1)
  await page?.screenshot({ path: path.join(output, 'failure.png') })
  console.error(await page?.locator('body').innerText())
  throw error
} finally {
  await writeFile(path.join(output, 'layout-result.json'), JSON.stringify(results, null, 2))
  await browser?.close()
  server.kill()
}
