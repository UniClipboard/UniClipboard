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
    // Receive failure modes: opaque-ID parsing, original/current index confusion,
    // stale metadata after save/retry, lossy 64-bit versions, and bytes-as-success.
    let receiveStage = 0
    let raceMetadataReads = 0
    let raceSnapshotReads = 0
    const receiveSnapshot = id => {
      const entryId = id
      if (id === 'receive-race') {
        raceSnapshotReads++
        id = 'receive-files'
      }
      const names = id === 'receive-files' ? 3 : id === 'receive-saving' ? 2 : 1
      const unknown = id === 'receive-unknown'
      const zero = id === 'receive-zero'
      const directory = id === 'receive-directory'
      const items = Array.from({ length: names }, (_, index) => ({
        itemId: `opaque-${['z', 'a', 'm'][index]}`,
        kind: directory ? 'directory_unit' : 'file',
        content: unknown ? null : directory ? { kind: 'directory_unit' } : { kind: 'file', index },
        historyFileIndex:
          id === 'receive-files' ? (receiveStage ? [0, 2, 1] : [1, 2, 0])[index] : zero ? 0 : null,
        state:
          id === 'receive-files' || id === 'receive-race'
            ? index === 1
              ? 'failed'
              : index === 2 || receiveStage
                ? 'completed'
                : 'transferring'
            : zero || unknown
              ? 'completed'
              : 'transferring',
        failureReason: id === 'receive-files' && index === 1 ? 'network_unavailable' : null,
        transfer: unknown
          ? null
          : {
              receivedBytes: zero
                ? '0'
                : id === 'receive-saving' || index === 2 || receiveStage
                  ? '1000'
                  : index === 0
                    ? '250'
                    : '500',
              totalBytes: zero ? '0' : id === 'receive-saving' && index === 1 ? null : '1000',
              round: receiveStage,
            },
      }))
      return {
        entryId,
        attemptId: `${entryId}-attempt`,
        state: zero || unknown ? 'completed' : 'receiving',
        snapshotVersion: unknown
          ? null
          : {
              producerId: 'receive-producer',
              attemptRevision: '9007199254740993',
              snapshotRevision: String(9007199254740992n + BigInt(receiveStage)),
            },
        totalBytes: names * 1000,
        completedBytes: zero ? 0 : 1000,
        itemsTotal: names,
        itemsCompleted: zero || unknown ? names : 1,
        items: id === 'receive-files' ? [items[2], items[0], items[1]] : items,
      }
    }
    await page.route('**/clipboard/**', async route => {
      const url = new URL(route.request().url())
      if (url.pathname.endsWith('/receive/retry')) {
        retryBody = route.request().postDataJSON()
        return route.fulfill({ json: { data: { outcome: 'accepted' }, ts: 1 } })
      }
      const detail = url.pathname.match(/\/entries\/(receive-[^/]+)$/)
      if (detail) {
        const id = detail[1]
        if (id === 'receive-race') {
          raceMetadataReads++
          receiveStage = 1
        }
        const names =
          id === 'receive-files' || id === 'receive-race'
            ? receiveStage
              ? ['A.zip', 'C.pdf', 'B.bin']
              : ['C.pdf', 'A.zip', 'B.bin']
            : ['空文件.txt']
        return route.fulfill({
          json: {
            data: {
              id,
              mimeType: 'text/uri-list',
              content: names.map(name => `file:///fixture/${encodeURIComponent(name)}`).join('\n'),
              sizeBytes: 3000,
            },
            ts: 1,
          },
        })
      }
      const match = url.pathname.match(/\/entries\/([^/]+)\/(resource|delivery|receive)$/)
      if (!match) return route.fulfill({ json: { data: null, ts: 1 } })
      const [, id, kind] = match
      if (id.startsWith('receive-'))
        return route.fulfill({
          json: { data: kind === 'receive' ? receiveSnapshot(id) : null, ts: 1 },
        })
      const count = {
        single: 1,
        image: 1,
        mixed: 4,
        gallery: 3,
        failed: 4,
        active: 2,
        local: 1,
        'outbound-indexed': 3,
        'outbound-single': 1,
        'outbound-multi': 3,
        'outbound-unknown': 2,
        'outbound-finished': 3,
      }[id]
      const snapshot =
        id === 'local' || id.startsWith('outbound-')
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
            ? id.startsWith('outbound-')
              ? {
                  entryId: id,
                  source: { tag: 'local' },
                  deliveries: ['macbook', 'work-pc', 'pixel'].map((targetDeviceId, i) => ({
                    targetDeviceId,
                    targetDeviceName: ['MacBook Pro', '工作电脑', 'Pixel 9'][i],
                    status: { tag: 'pending' },
                    reasonDetail: null,
                    updatedAtMs: null,
                  })),
                }
              : null
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
    // Failure modes: file/target association is lost, target progress is averaged,
    // an unknown byte count becomes zero, or a 100% processing item claims success.
    for (const scenario of [
      'outbound-single',
      'outbound-multi',
      'outbound-unknown',
      'outbound-finished',
    ]) {
      await page.getByRole('button', { name: scenario, exact: true }).click()
      const panel = page.getByTestId('file-preview')
      const files = panel.locator('[data-outbound-file]')
      const count = scenario === 'outbound-single' ? 1 : scenario === 'outbound-unknown' ? 2 : 3
      await files.first().waitFor({ state: 'attached' })
      assert.equal(await files.count(), count)
      assert.equal(
        await panel.locator('[title="发布素材.zip"]').count(),
        scenario === 'outbound-unknown' ? 0 : 1
      )
      assert.equal(
        await page.getByTestId('clipboard-detail').locator('[data-outbound-state]').count(),
        0
      )
      if (scenario === 'outbound-single') {
        assert.equal(await panel.locator('details').count(), 0)
        const bar = files.getByRole('progressbar')
        const value = Number(await bar.getAttribute('aria-valuenow'))
        const max = Number((await bar.getAttribute('aria-valuemax')) ?? 100)
        assert.equal(value / max, 0.5)
      }
      if (scenario === 'outbound-multi') {
        assert.equal(await panel.locator('[data-file-target-state]').count(), 9)
        const bars = files.nth(0).getByRole('progressbar')
        const values = []
        for (const bar of await bars.all())
          values.push(
            Number(await bar.getAttribute('aria-valuenow')) /
              Number((await bar.getAttribute('aria-valuemax')) ?? 100)
          )
        assert.deepEqual(values, [0.5, 0.25, 1])
        await files.nth(0).getByText('MacBook Pro', { exact: true }).waitFor()
        await files.nth(0).getByText('工作电脑', { exact: true }).waitFor()
        await files.nth(0).getByText('Pixel 9', { exact: true }).waitFor()
        const unknown = files.nth(1).locator('[data-file-target-state="waiting"]')
        assert.equal(await unknown.getByRole('progressbar').getAttribute('aria-valuenow'), null)
        await unknown.getByText('未知 / 总大小未知', { exact: true }).waitFor()
        await files.nth(1).getByText('网络不可用', { exact: true }).waitFor()
        await files.nth(1).getByText('已由你取消', { exact: true }).waitFor()
        assert.equal(await files.nth(2).locator('[data-file-target-state="processing"]').count(), 3)
        assert.equal(await files.nth(2).locator('[data-file-target-state="completed"]').count(), 0)
        const summary = panel.locator('details').first().locator('summary')
        await summary.click()
        assert.equal(await files.nth(0).isVisible(), false)
        await summary.press('Enter')
        assert.equal(await files.nth(0).isVisible(), true)
      }
      if (scenario === 'outbound-unknown') {
        assert.equal(await files.nth(0).getByRole('progressbar').count(), 0)
        assert.equal(await files.nth(1).getByRole('progressbar').count(), 0)
        await files.nth(1).getByText('未知 / 总大小未知', { exact: true }).waitFor()
      }
      if (scenario === 'outbound-finished') {
        await panel.getByText('已送达 2 / 3 个设备 · 1 个设备失败', { exact: true }).waitFor()
        assert.equal(await files.nth(1).locator('[data-file-target-state="failed"]').count(), 1)
      }
      const width = await panel.evaluate(el => el.scrollWidth <= el.clientWidth + 1)
      assert.equal(width, true, `${platform}/${scheme}/${scenario} overflows horizontally`)
      await page.screenshot({ path: path.join(output, `${platform}-${scheme}-${scenario}.png`) })
      results.push({ scenario, passed: true, fileCount: count })
    }
    // Exercise the production detail's file_names forwarding, not a status-only fixture.
    await page.getByRole('button', { name: 'outbound-indexed', exact: true }).click()
    const indexedStatus = page.getByTestId('delivery-status-macbook')
    await indexedStatus.waitFor()
    await indexedStatus.hover()
    const indexedDetails = page.getByTestId('delivery-transfer-details')
    await indexedDetails.getByText('gamma.txt', { exact: true }).waitFor()
    const indexedRows = indexedDetails.locator('li')
    assert.equal(await indexedRows.count(), 5)
    const labels = ['gamma.txt', 'alpha.txt', '文件 10', '内容 1', '文件夹 1']
    for (const [index, label] of labels.entries())
      await indexedRows.nth(index).getByText(label, { exact: true }).waitFor()
    assert.equal(await indexedDetails.getByText('beta.txt', { exact: true }).count(), 0)
    await indexedRows.nth(1).getByText('已由你取消', { exact: true }).waitFor()
    await indexedRows.nth(2).getByText('未知原因', { exact: true }).waitFor()
    await indexedRows.nth(3).getByText('网络不可用', { exact: true }).waitFor()
    await indexedRows.nth(4).getByText('对方已取消', { exact: true }).waitFor()
    assert.equal(await indexedDetails.getByRole('progressbar').count(), 0)
    await page.screenshot({ path: path.join(output, `${platform}-${scheme}-outbound-indexed.png`) })
    results.push({ scenario: 'outbound-indexed', passed: true, labels })
    await indexedStatus.focus()
    await page.keyboard.press('Escape')
    await indexedDetails.waitFor({ state: 'hidden' })
    for (const scenario of [
      'receive-files',
      'receive-saving',
      'receive-zero',
      'receive-directory',
      'receive-unknown',
      'receive-race',
    ]) {
      if (scenario === 'receive-race') receiveStage = 0
      await page.getByRole('button', { name: scenario, exact: true }).click()
      const panel = page.getByTestId('file-preview')
      const files = panel.locator('[data-receive-file-index]')
      if (scenario === 'receive-files') {
        await panel.locator('[title="A.zip"]').waitFor()
        assert.equal(await files.count(), 3)
        for (const [index, name] of ['A.zip', 'B.bin', 'C.pdf'].entries()) {
          assert.equal(
            await panel
              .locator('li')
              .filter({ has: page.locator(`[data-receive-file-index="${index}"]`) })
              .locator(`[title="${name}"]`)
              .count(),
            1
          )
        }
        await files.nth(0).getByText('接收中 · 25%', { exact: true }).waitFor()
        await files.nth(1).getByText('失败 · 50%', { exact: true }).waitFor()
        await page.screenshot({
          path: path.join(output, `${platform}-${scheme}-receive-in-progress.png`),
        })
        receiveStage = 1
        await page.getByRole('button', { name: 'refresh receive', exact: true }).click()
        await files.nth(0).getByText('已保存 · 100%', { exact: true }).waitFor()
        assert.equal(
          await panel
            .locator('li')
            .filter({ has: page.locator('[data-receive-file-index="0"]') })
            .locator('[title="A.zip"]')
            .count(),
          1
        )
        assert.equal(
          await panel
            .locator('li')
            .filter({ has: page.locator('[data-receive-file-index="2"]') })
            .locator('[title="C.pdf"]')
            .count(),
          1
        )
        receiveStage = 0
        const [stale] = await Promise.all([
          page.waitForResponse(response =>
            new URL(response.url()).pathname.endsWith('/receive-files/receive')
          ),
          page.getByRole('button', { name: 'refresh receive', exact: true }).click(),
        ])
        await stale.finished()
        await page.evaluate(
          () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))
        )
        assert.equal(await files.nth(0).getAttribute('data-receive-file-state'), 'completed')
      } else if (scenario === 'receive-saving') {
        await files.first().waitFor()
        await files.nth(0).getByText('接收中 · 100%', { exact: true }).waitFor()
        assert.equal(await files.nth(0).getAttribute('data-receive-file-state'), 'transferring')
        assert.equal(
          await files.nth(1).getByRole('progressbar').getAttribute('aria-valuenow'),
          null
        )
        await panel.locator('[title="素材.zip"]').waitFor()
      } else if (scenario === 'receive-zero') {
        await files.first().waitFor()
        assert.equal(await files.first().getByRole('progressbar').count(), 0)
        await files.first().getByText('0 字节 / 0 字节', { exact: true }).waitFor()
      } else if (scenario === 'receive-directory') {
        await panel.getByText('整个文件集', { exact: true }).waitFor()
        assert.equal(await files.count(), 0)
        assert.equal(await panel.getByRole('progressbar').count(), 1)
      } else if (scenario === 'receive-race') {
        await panel.locator('[title="A.zip"]').waitFor()
        await files.nth(0).getByText('已保存 · 100%', { exact: true }).waitFor()
        assert.ok(raceSnapshotReads >= 3)
        assert.ok(raceMetadataReads >= 2)
        assert.equal(
          await panel
            .locator('li')
            .filter({ has: page.locator('[data-receive-file-index="0"]') })
            .locator('[title="A.zip"]')
            .count(),
          1
        )
      } else {
        await panel.getByRole('status').waitFor()
        assert.equal(await files.count(), 0)
        assert.equal(await panel.getByRole('progressbar').count(), 0)
      }
      assert.equal(await panel.evaluate(el => el.scrollWidth > el.clientWidth + 1), false)
      await page.screenshot({ path: path.join(output, `${platform}-${scheme}-${scenario}.png`) })
      results.push({ scenario, passed: true })
    }
    assert.deepEqual(errors, [])
    await writeFile(path.join(output, `${scheme}.json`), JSON.stringify(results, null, 2))
  })
}
