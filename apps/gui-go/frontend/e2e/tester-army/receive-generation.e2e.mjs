import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, outputFor } from './page.mjs'

// Failure modes: a retired receive response or confirmed metadata response
// overwrites the same entry after a new detail effect has already published.
for (const phase of ['receive', 'confirm']) {
  test(`receive generation fence: late ${phase} response after detail remount`, async ({
    app,
    page,
  }) => {
    let reads = 0
    let release
    let held
    const blocked = new Promise(resolve => {
      held = resolve
    })
    const snapshot = percent => ({
      entryId: 'receive-files',
      attemptId: 'receive-files-attempt',
      state: 'receiving',
      totalBytes: 1000,
      completedBytes: percent * 10,
      itemsTotal: 1,
      itemsCompleted: 0,
      // The compatibility API may omit revisions. Lifecycle fencing must still work.
      snapshotVersion: null,
      items: [
        {
          itemId: 'opaque',
          kind: 'file',
          state: 'transferring',
          failureReason: null,
          content: { kind: 'file', index: 0 },
          historyFileIndex: phase === 'confirm' ? 0 : null,
          transfer: { receivedBytes: String(percent * 10), totalBytes: '1000', round: 0 },
        },
      ],
    })
    await page.route('**/clipboard/**', async route => {
      const url = new URL(route.request().url())
      let data = null
      if (url.pathname === '/clipboard/entries/receive-files/receive') {
        reads++
        if (reads === (phase === 'receive' ? 1 : 2)) {
          held()
          await new Promise(resolve => {
            release = resolve
          })
          data = snapshot(75)
        } else data = snapshot(25)
      } else if (url.pathname === '/clipboard/entries/receive-files') {
        data = {
          id: 'receive-files',
          mimeType: 'text/uri-list',
          content: 'file:///fixture/current.bin',
          sizeBytes: 1000,
        }
      }
      await route.fulfill({ json: { data, ts: 1 } })
    })
    await page.goto(app.baseUrl)
    await page.getByRole('button', { name: 'receive-files', exact: true }).click()
    await blocked
    await page.getByRole('button', { name: 'local', exact: true }).click()
    await page.getByRole('button', { name: 'receive-files', exact: true }).click()
    const bar = page.getByTestId('file-preview').getByRole('progressbar')
    await bar.waitFor()
    await page.waitForFunction(
      () => document.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow') === '25'
    )
    const late = page.waitForResponse(
      async response =>
        response.url().includes('/receive') && (await response.json()).data?.completedBytes === 750
    )
    release()
    await late
    // Drain response continuations and the React commit before observing the value.
    await page.evaluate(
      () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))
    )
    assert.equal(
      await bar.getAttribute('aria-valuenow'),
      '25',
      'retired reply overwrote current detail'
    )
    const output = await outputFor('receive-generation')
    await page.screenshot({ path: path.join(output, `${phase}.png`) })
    await writeFile(
      path.join(output, `${phase}.json`),
      JSON.stringify({ phase, reads, passed: true, latePercent: 75, retainedPercent: 25 }, null, 2)
    )
  })
}
