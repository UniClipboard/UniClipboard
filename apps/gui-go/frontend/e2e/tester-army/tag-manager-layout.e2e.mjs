import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, installSession } from './backend-page.mjs'
import { backendOutput as output, backendPlatform } from './backend-suites.mjs'

test('tag-manager-layout: original real-backend assertions', async ({
  app,
  backend: daemon,
  createContext,
}) => {
  const conn = { token: daemon.fileToken }
  let lastPage
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
  const results = []
  try {
    for (const colorScheme of ['light', 'dark']) {
      for (const width of [1280, 760, 640]) {
        const context = await createContext({ viewport: { width, height: 800 }, colorScheme })
        const page = await context.newPage()
        lastPage = page
        await installSession(page, daemon, token)
        await page.addInitScript(() => localStorage.setItem('uniclipboard.language', 'zh-CN'))
        await page.goto(`${app.baseUrl}/history.html`)
        const consent = page.getByRole('button', { name: '关闭', exact: true })
        const more = page.getByRole('button', { name: '更多', exact: true }).first()
        await consent.or(more).first().waitFor()
        if (await consent.isVisible()) await consent.click()
        await more.click()
        await page.getByRole('menuitem', { name: '管理标签', exact: true }).click()
        const family = backendPlatform === 'linux' ? 'adw' : 'mac'
        const header = page.locator(`.${family}-sheet-header`)
        await header.waitFor()
        await page.waitForTimeout(350)
        const geometry = await header.evaluate((element, family) => {
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
            count: measure(element.querySelector(`.${family}-text`)),
            sort: measure(element.querySelector(`.${family}-popup-btn`).firstElementChild),
            search: measure(element.querySelector(`.${family}-search`)),
            overflow: element.scrollWidth > element.clientWidth,
          }
        }, family)
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
    const page = lastPage
    await page?.screenshot({ path: path.join(output, 'failure.png') })
    console.error(await page?.locator('body').innerText())
    throw error
  } finally {
    await writeFile(path.join(output, 'layout-result.json'), JSON.stringify(results, null, 2))
  }
})
