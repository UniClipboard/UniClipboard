import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, outputFor } from './page.mjs'
import { selectedPlatform as platform } from './suites.mjs'
for (const scheme of ['light', 'dark']) {
  test(`file-preview ${scheme}`, async ({ app, page }) => {
    const base = app.baseUrl.replace(/\/$/, '')
    const output = await outputFor('file-preview')
    const results = []
    const errors = []
    page.on('pageerror', error => {
      errors.push(error.message)
      console.error(error.message)
    })
    let retryBody
    await page.route('**/clipboard/**', async route => {
      const url = new URL(route.request().url())
      if (url.pathname.endsWith('/receive/retry')) {
        retryBody = route.request().postDataJSON()
        return route.fulfill({ json: { data: { outcome: 'accepted' }, ts: 1 } })
      }
      const match = url.pathname.match(/\/entries\/([^/]+)\/(resource|delivery|receive)$/)
      if (!match) return route.fulfill({ json: { data: null, ts: 1 } })
      const [, id, kind] = match
      const count = {
        single: 1,
        image: 1,
        mixed: 4,
        gallery: 3,
        failed: 4,
        active: 2,
        local: 1,
      }[id]
      const snapshot =
        id === 'local'
          ? null
          : {
              entryId: id,
              attemptId: `${id}-attempt`,
              state: id === 'failed' ? 'failed' : id === 'active' ? 'receiving' : 'completed',
              totalBytes: count * 2048,
              completedBytes: id === 'failed' ? 6144 : count * 2048,
              itemsTotal: count,
              itemsCompleted: id === 'failed' ? 3 : count,
              items: Array.from({ length: count }, (_, i) => ({
                itemId: `${id}-opaque-${i}`,
                kind: 'file',
                state:
                  id === 'failed' && i === 1
                    ? 'failed'
                    : id === 'active'
                      ? 'transferring'
                      : 'completed',
                failureReason: id === 'failed' && i === 1 ? 'storage_unavailable' : null,
              })),
            }
      const data =
        kind === 'receive'
          ? snapshot
          : kind === 'delivery'
            ? null
            : {
                blobId: null,
                mimeType: id === 'image' ? 'image/svg+xml' : 'text/uri-list',
                sizeBytes: 2048,
                url: id === 'image' ? '/fixture-image.svg' : null,
                inlineData: null,
              }
      return route.fulfill({ json: { data, ts: 1 } })
    })
    await page.route(/\/(fixture-image\.svg|host-file)/, route =>
      route.fulfill({
        contentType: 'image/svg+xml',
        body: '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="400"><rect width="600" height="400" fill="#527a78"/><circle cx="400" cy="120" r="60" fill="#e8d9ac"/><path d="M0 400 180 100 400 400" fill="#234c4c"/></svg>',
      })
    )
    await page.goto(`${base}/?scheme=${scheme}`)
    for (const scenario of ['single', 'image', 'mixed', 'gallery', 'failed', 'active', 'local']) {
      await page.getByRole('button', { name: scenario, exact: true }).click()
      const panel = page.getByTestId('file-preview')
      await panel.waitFor()
      if (scenario !== 'local') await panel.getByRole('status').waitFor()
      assert.equal(await page.getByText('接收的文件', { exact: true }).count(), 0)
      assert.equal(
        await panel.locator(':scope > section > ul > li').count(),
        { single: 1, image: 1, mixed: 4, gallery: 3, failed: 4, active: 2, local: 1 }[scenario]
      )
      if (scenario === 'active') {
        const progress = panel.getByRole('progressbar')
        const value = Number(await progress.getAttribute('aria-valuenow'))
        const max = Number((await progress.getAttribute('aria-valuemax')) ?? 1)
        assert.equal(value / max, 0.25)
        await panel.getByRole('button', { name: '取消传输' }).waitFor()
      }
      if (scenario === 'failed') {
        await panel.getByText('已保存 3/4 · 1 项失败', { exact: true }).waitFor()
        await panel.getByText('接收详情', { exact: true }).click()
        await panel.getByText('失败 · 存储不可用', { exact: true }).waitFor()
        await panel.getByRole('button', { name: '重试失败项' }).click()
        await panel.getByText('已开始重试。文件完成保存后才算成功。', { exact: true }).waitFor()
        assert.deepEqual(retryBody, {
          expectedAttemptId: 'failed-attempt',
          itemIds: ['failed-opaque-1'],
        })
      }
      const clipping = await panel.evaluate(
        el => el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 2
      )
      assert.equal(clipping, false, `${platform}/${scheme}/${scenario} clips`)
      await page.screenshot({
        path: path.join(output, `${platform}-${scheme}-${scenario}.png`),
      })
    }
    assert.deepEqual(errors, [])
    await writeFile(path.join(output, `${scheme}.json`), JSON.stringify(results, null, 2))
  })
}
