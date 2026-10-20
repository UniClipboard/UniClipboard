import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const frontend = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const output = path.resolve(
  process.argv[2] || path.join(frontend, '../../../target/e2e/history-detail-regressions')
)
await mkdir(output, { recursive: true })
const browser = await chromium.launch({
  headless: true,
  ...(process.env.PLAYWRIGHT_EXECUTABLE_PATH
    ? { executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH }
    : {}),
})
const results = []
const fullText = id =>
  Array.from({ length: 120 }, (_, i) => `${id} LINE ${i + 1}: readable clipboard content`).join(
    '\n'
  )
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))
try {
  for (const platform of (process.env.UI_KIT_PLATFORMS || 'linux,macos,windows').split(',')) {
    const server = spawn(process.execPath, ['e2e/visual-effects-server.mjs'], {
      cwd: frontend,
      env: {
        ...process.env,
        UC_UI_PLATFORM: platform,
        UI_FIXTURE_ENTRY: 'e2e/fixtures/history-detail-regressions.tsx',
        UI_FIXTURE_PORT: '1468',
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let serverLog = ''
    try {
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(serverLog)), 60000)
        server.stdout.on('data', c => {
          serverLog += c
          if (serverLog.includes('Local:')) {
            clearTimeout(timer)
            resolve()
          }
        })
        server.stderr.on('data', c => {
          serverLog += c
        })
        server.on('exit', code => {
          clearTimeout(timer)
          reject(new Error(`server ${code}: ${serverLog}`))
        })
      })
      for (const scheme of ['light', 'dark']) {
        const page = await browser.newPage({
          viewport: { width: 850, height: 750 },
          reducedMotion: 'reduce',
        })
        const errors = []
        page.setDefaultTimeout(8000)
        page.on('pageerror', e => {
          errors.push(e.message)
          console.error('pageerror', e.message)
        })
        await page.route('**/clipboard/**', async route => {
          const url = new URL(route.request().url())
          const match = url.pathname.match(/\/entries\/([^/]+)\/(resource|delivery)/)
          if (match) {
            const [, id, kind] = match
            await pause(200)
            const data =
              kind === 'delivery'
                ? {
                    entryId: id,
                    source: { tag: 'local' },
                    deliveries: [
                      {
                        targetDeviceId: 'peer',
                        targetDeviceName: `Peer ${id}`,
                        status: { tag: 'delivered' },
                        reasonDetail: null,
                        updatedAtMs: 1,
                      },
                    ],
                  }
                : {
                    blobId: null,
                    mimeType: id === 'image' ? 'image/png' : 'text/plain',
                    sizeBytes: 1024,
                    url: id === 'image' ? '/clipboard/blobs/image' : null,
                    inlineData:
                      id === 'image' ? null : Buffer.from(fullText(id)).toString('base64'),
                  }
            return route.fulfill({ json: { data, ts: Date.now() } })
          }
          if (url.pathname === '/clipboard/blobs/image') {
            await pause(700)
            // Synthetic SVG bytes served with their own MIME type; no real user content.
            return route.fulfill({
              contentType: 'image/svg+xml',
              body: '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="800"><rect width="600" height="800" fill="#3584e4"/></svg>',
            })
          }
          await route.fulfill({ json: { data: [], ts: Date.now() } })
        })
        const check = async (name, run) => {
          try {
            await run()
            results.push({ platform, scheme, name, ok: true })
          } catch (error) {
            results.push({ platform, scheme, name, ok: false, error: error.message })
            await page.screenshot({ path: path.join(output, `${platform}-${scheme}-${name}.png`) })
          }
        }
        await page.goto(`http://127.0.0.1:1468/?scheme=${scheme}`)
        const detail = page.getByTestId('clipboard-detail')
        await detail.waitFor()
        console.log(platform, scheme, 'detail mounted')
        await page.getByText('Peer text-a', { exact: true }).waitFor()
        console.log('delivery ready')
        await page.getByText(/text-a LINE 120/).waitFor()
        console.log('text ready')
        await check('long-text-reachable', async () => {
          const reachable = await detail.evaluate(root => {
            const text = [...root.querySelectorAll('p')].find(p =>
              p.textContent.includes('LINE 120')
            )
            let el = text
            while (el && el !== root) {
              if (/(auto|scroll)/.test(getComputedStyle(el).overflowY))
                el.scrollTop = el.scrollHeight
              el = el.parentElement
            }
            const rects = text.getClientRects()
            const end = document.createRange()
            end.setStart(text.firstChild, text.firstChild.length - 8)
            end.setEnd(text.firstChild, text.firstChild.length)
            const last = end.getBoundingClientRect()
            el = text
            while (el && el !== root) {
              const style = getComputedStyle(el)
              const box = el.getBoundingClientRect()
              if (
                /(hidden|auto|scroll)/.test(style.overflowY) &&
                (last.bottom > box.bottom + 2 || last.top < box.top - 2)
              )
                return false
              el = el.parentElement
            }
            return last.bottom <= innerHeight && rects.length > 0
          })
          assert.ok(reachable, 'last text line is clipped even after scrolling every ancestor')
        })
        await page.getByRole('button', { name: 'text-b', exact: true }).click()
        await page.getByText('Peer text-b', { exact: true }).waitFor()
        await page.getByText(/text-b LINE 120/).waitFor()
        await check('warm-switch-stable', async () => {
          await page.evaluate(() => {
            window.__detailFrames = []
            window.__detailSampling = true
            const sample = () => {
              if (!window.__detailSampling) return
              const root = document.querySelector('[data-testid="clipboard-detail"]')
              window.__detailFrames.push({
                id: root?.getAttribute('data-entry-id'),
                text: root?.textContent,
              })
              requestAnimationFrame(sample)
            }
            requestAnimationFrame(sample)
          })
          await page.getByRole('button', { name: 'text-a', exact: true }).click()
          await pause(500)
          const frames = await page.evaluate(() => {
            window.__detailSampling = false
            return window.__detailFrames.filter(f => f.id === 'text-a')
          })
          assert.ok(frames.length > 0)
          assert.ok(
            frames.every(
              f =>
                !f.text.includes('加载中') &&
                f.text.includes('text-a LINE 120') &&
                f.text.includes('Peer text-a') &&
                !f.text.includes('Peer text-b')
            ),
            'warm selection painted loading, missing content/delivery, or stale delivery'
          )
        })
        await check('same-item-refresh-stable', async () => {
          await page.evaluate(() => {
            window.__removedPreview = false
            const root = document.querySelector('[data-testid="clipboard-detail"]')
            window.__observer = new MutationObserver(() => {
              if (root.textContent.includes('加载中')) window.__removedPreview = true
            })
            window.__observer.observe(root, { subtree: true, childList: true, characterData: true })
          })
          await page.getByRole('button', { name: 'refresh item' }).click()
          await pause(300)
          assert.equal(
            await page.evaluate(() => {
              window.__observer.disconnect()
              return window.__removedPreview
            }),
            false,
            'same entry object replacement clears loaded preview'
          )
        })
        await check('image-facts-stable', async () => {
          await page.getByRole('button', { name: 'image', exact: true }).click()
          const facts = detail.locator('dl').first()
          await facts.waitFor()
          const labelsBefore = await facts.locator('dt').allTextContents()
          const formatBefore = await facts
            .locator('div')
            .filter({ has: page.locator('dt', { hasText: '格式' }) })
            .first()
            .boundingBox()
          await detail.locator('img').first().waitFor()
          await page.getByText('600 × 800', { exact: true }).waitFor()
          const labelsAfter = await facts.locator('dt').allTextContents()
          const formatAfter = await facts
            .locator('div')
            .filter({ has: page.locator('dt', { hasText: '格式' }) })
            .first()
            .boundingBox()
          assert.deepEqual(labelsBefore, labelsAfter, 'fact cards were inserted after image decode')
          assert.ok(
            Math.abs(formatBefore.x - formatAfter.x) < 1 &&
              Math.abs(formatBefore.y - formatAfter.y) < 1,
            'format card moved'
          )
        })
        await check('warm-image-stable', async () => {
          await page.getByRole('button', { name: 'text-a', exact: true }).click()
          await page.getByText('Peer text-a', { exact: true }).waitFor()
          await page.evaluate(() => {
            window.__imageFrames = []
            window.__detailSampling = true
            const sample = () => {
              if (!window.__detailSampling) return
              const root = document.querySelector('[data-testid="clipboard-detail"]')
              if (root?.getAttribute('data-entry-id') === 'image') {
                const img = root.querySelector('img')
                window.__imageFrames.push({
                  present: Boolean(img),
                  complete: img?.complete,
                  width: img?.naturalWidth,
                  text: root.textContent.slice(0, 160),
                })
              }
              requestAnimationFrame(sample)
            }
            requestAnimationFrame(sample)
          })
          await page.getByRole('button', { name: 'image', exact: true }).click()
          await pause(400)
          const frames = await page.evaluate(() => {
            window.__detailSampling = false
            return window.__imageFrames
          })
          assert.ok(
            frames.length > 0 &&
              frames.every(frame => frame.present) &&
              frames.at(-1).complete &&
              frames.at(-1).width > 0,
            `cached image painted an empty/loading frame: ${JSON.stringify(frames.slice(0, 4))}`
          )
        })
        await check('selection-scroll-reset', async () => {
          await page.setViewportSize({ width: 460, height: 440 })
          await page.getByRole('button', { name: 'text-a', exact: true }).click()
          await page.getByText('Peer text-a', { exact: true }).waitFor()
          await detail.evaluate(root => {
            for (const el of root.querySelectorAll('*'))
              if (/(auto|scroll)/.test(getComputedStyle(el).overflowY))
                el.scrollTop = el.scrollHeight
          })
          await page.getByRole('button', { name: 'text-b', exact: true }).click()
          await pause(300)
          await page.getByRole('button', { name: 'text-a', exact: true }).click()
          const positions = await detail.evaluate(root =>
            [...root.querySelectorAll('*')]
              .filter(el => /(auto|scroll)/.test(getComputedStyle(el).overflowY))
              .map(el => el.scrollTop)
          )
          assert.ok(
            positions.every(top => top === 0),
            `selection retained scroll: ${positions}`
          )
        })
        await check('page-errors', async () => assert.deepEqual(errors, []))
        await page.screenshot({ path: path.join(output, `${platform}-${scheme}-final.png`) })
        await page.close()
      }
    } finally {
      server.kill()
      await new Promise(resolve =>
        server.exitCode !== null ? resolve() : server.once('exit', resolve)
      )
    }
  }
} finally {
  await browser.close()
  await writeFile(path.join(output, 'results.json'), JSON.stringify(results, null, 2))
  console.log(JSON.stringify(results, null, 2))
}
assert.ok(results.length > 0 && results.every(r => r.ok), 'detail UI regressions remain')
