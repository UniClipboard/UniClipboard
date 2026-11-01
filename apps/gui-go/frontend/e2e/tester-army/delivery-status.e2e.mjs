import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, outputFor } from './page.mjs'
import { selectedPlatform } from './suites.mjs'

// Failure cases: duplicate rows, premature success at 100%, unknown totals,
// stale activity, missing per-file details, overflow, focus access and duplicate retry.
for (const scheme of ['light', 'dark']) {
  test(`delivery-status ${scheme}`, async ({ app, page }) => {
    const output = await outputFor('delivery-status')
    const errors = []
    const requests = []
    let releaseRetry
    const retryGate = new Promise(resolve => {
      releaseRetry = resolve
    })
    page.on('pageerror', error => errors.push(error.message))
    await page.route('**/clipboard/resend*', async route => {
      requests.push(route.request().postDataJSON())
      await retryGate
      await route.fulfill({
        json: {
          data: { accepted: 1, duplicate: 0, offline: 0, errored: 0, pending: 0, reconciled: [] },
          ts: 1,
        },
      })
    })
    await page.goto(`${app.baseUrl.replace(/\/$/, '')}/?scheme=${scheme}`)
    const list = page.getByRole('list', { name: '同步状态' })
    await list.waitFor()
    assert.equal(await list.locator(':scope > li').count(), 2)
    assert.equal(await page.locator('[data-outbound-state]').count(), 0)
    const status = page.getByTestId('delivery-status-phone')
    await status.hover()
    const detail = page.getByTestId('delivery-transfer-details')
    await detail.waitFor()
    assert.equal(await detail.getAttribute('data-transfer-skin'), selectedPlatform)
    assert.equal(await detail.getByRole('progressbar').count(), 12)
    assert.equal(await detail.locator('li').count(), 12)
    await detail.getByText('transfer-file-12.bin', { exact: true }).waitFor()
    assert.equal(await status.getByRole('progressbar').getAttribute('aria-valuenow'), '25')
    await detail.hover()
    assert.equal(await detail.isVisible(), true)
    const files = detail.getByRole('list')
    assert.equal(await files.evaluate(el => el.scrollHeight > el.clientHeight), true)
    assert.equal(
      await detail.evaluate(el => {
        const box = el.getBoundingClientRect()
        return el.contains(document.elementFromPoint(box.x + box.width / 2, box.bottom - 12))
      }),
      true
    )
    await page.screenshot({ path: path.join(output, `${scheme}-multi-file.png`) })
    const viewport = page.viewportSize()
    await page.setViewportSize({ width: 360, height: 650 })
    await status.hover()
    await detail.waitFor()
    await detail.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)))
    const bounds = await detail.boundingBox()
    assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= 360)
    assert.equal(await files.evaluate(el => el.scrollWidth <= el.clientWidth), true)
    await page.screenshot({ path: path.join(output, `${scheme}-narrow.png`) })
    await page.setViewportSize(viewport)
    await page.getByRole('button', { name: 'bytes-complete', exact: true }).click()
    assert.equal(await status.getByRole('progressbar').getAttribute('aria-valuenow'), '100')
    await page.getByRole('button', { name: 'completed', exact: true }).click()
    assert.equal(await status.getByRole('progressbar').count(), 0)
    assert.match(await status.getAttribute('aria-label'), /已送达/)
    await detail.waitFor({ state: 'hidden' })
    await page.getByRole('button', { name: 'remote-failed', exact: true }).focus()
    await page.keyboard.press('Tab')
    assert.equal(await status.evaluate(el => el === document.activeElement), true)
    await detail.waitFor()
    assert.equal(await detail.getByRole('progressbar').count(), 0)
    await page.screenshot({ path: path.join(output, `${scheme}-completed.png`) })
    await page.keyboard.press('Escape')
    await detail.waitFor({ state: 'hidden' })
    await page.getByRole('button', { name: 'unknown', exact: true }).click()
    assert.equal(await status.getByRole('progressbar').getAttribute('aria-valuenow'), null)
    await page.getByRole('button', { name: 'stale', exact: true }).click()
    assert.equal(await status.getByRole('progressbar').count(), 0)
    assert.match(await status.getAttribute('aria-label'), /结果未确认/)
    await status.hover()
    await detail.waitFor()
    assert.equal(await detail.getByText('结果未确认', { exact: true }).count(), 13)
    await page.getByRole('button', { name: 'stale-completed', exact: true }).click()
    assert.match(await status.getAttribute('aria-label'), /结果未确认/)
    await page.getByRole('button', { name: 'zero', exact: true }).click()
    assert.match(await status.getAttribute('aria-label'), /已送达/)
    assert.equal(await status.getByRole('progressbar').count(), 0)
    await page.getByRole('button', { name: 'failed', exact: true }).click()
    assert.match(await status.getAttribute('aria-label'), /重发/)
    await detail.waitFor({ state: 'hidden' })
    await status.hover()
    await detail.getByText('网络不可用', { exact: true }).waitFor()
    assert.equal(await detail.getByRole('progressbar').count(), 0)
    assert.equal(await detail.getByText('完成', { exact: true }).count(), 11)
    await page.screenshot({ path: path.join(output, `${scheme}-failed.png`) })
    const response = page.waitForResponse(response =>
      new URL(response.url()).pathname.endsWith('/clipboard/resend')
    )
    await Promise.all([
      page.waitForRequest(request => new URL(request.url()).pathname.endsWith('/clipboard/resend')),
      status.click(),
    ])
    assert.equal(await status.getAttribute('aria-disabled'), 'true')
    await page.keyboard.press('Enter')
    assert.equal(requests.length, 1)
    releaseRetry()
    await response
    assert.deepEqual(requests, [
      { entryId: 'delivery-entry', peers: ['phone'], confirmNewTransfer: false },
    ])
    await page.getByRole('button', { name: 'no-snapshot', exact: true }).click()
    assert.match(await status.getAttribute('aria-label'), /已接收/)
    await page.getByRole('button', { name: 'remote-failed', exact: true }).click()
    assert.equal(await status.getAttribute('aria-disabled'), 'true')
    assert.deepEqual(errors, [])
    await writeFile(
      path.join(output, `${scheme}.json`),
      JSON.stringify({ scheme, assertions: 'passed', retryRequests: requests, errors }, null, 2)
    )
  })
}

for (const scheme of ['light', 'dark']) {
  test(`delivery retry confirmation ${scheme}`, async ({ app, page }) => {
    const requests = []
    await page.route('**/paired-devices', route => route.fulfill({ json: { data: [], ts: 1 } }))
    await page.route('**/clipboard/resend*', async route => {
      const request = route.request().postDataJSON()
      requests.push(request)
      await route.fulfill({
        json: {
          data: {
            accepted: request.confirmNewTransfer ? 1 : 0,
            duplicate: 0,
            offline: 0,
            errored: 0,
            pending: 0,
            reconciled: request.confirmNewTransfer
              ? []
              : [
                  {
                    targetDeviceId: 'phone',
                    state: 'settled',
                    newTransferRequested: false,
                    duplicateFilesPossible: true,
                  },
                ],
          },
          ts: 1,
        },
      })
    })
    await page.goto(`${app.baseUrl.replace(/\/$/, '')}/?scheme=${scheme}`)
    await page.getByRole('button', { name: 'failed', exact: true }).click()
    const status = page.getByTestId('delivery-status-phone')
    const confirmation = page.getByTestId('resend-confirm-dialog')
    await status.click()
    await confirmation.waitFor()
    assert.equal(requests.length, 1)
    assert.equal(requests[0].confirmNewTransfer, false)
    await page.keyboard.press('Escape')
    await confirmation.waitFor({ state: 'hidden' })
    assert.equal(requests.length, 1)
    await status.click()
    await confirmation.waitFor()
    const response = page.waitForResponse(
      response => response.request().postDataJSON()?.confirmNewTransfer === true
    )
    await page.getByTestId('resend-confirm-accept').click()
    await response
    await confirmation.waitFor({ state: 'hidden' })
    assert.deepEqual(
      requests.map(request => request.confirmNewTransfer),
      [false, false, true]
    )
    assert.ok(
      requests.every(
        request => request.entryId === 'delivery-entry' && request.peers.join() === 'phone'
      )
    )
    const output = await outputFor('delivery-status')
    await writeFile(
      path.join(output, `${scheme}-retry-confirmation.json`),
      JSON.stringify({ scheme, requests, assertions: 'passed' }, null, 2)
    )
  })
}
