import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import { test, installSession } from './backend-page.mjs'
import { backendOutput as output, backendPlatform as platform } from './backend-suites.mjs'

// Real HTTP/WS and paired Engines: no response interception or injected snapshots.
test('delivery-live: isolate two peers and confirm a failed file retry', async ({
  app,
  backend,
  createContext,
}) => {
  const { entryId, peerIds } = backend.delivery
  const context = await createContext({ viewport: { width: 1280, height: 900 } })
  const page = await context.newPage()
  await installSession(page, backend)
  const requests = []
  page.on('request', request => {
    if (new URL(request.url()).pathname === '/clipboard/resend')
      requests.push(request.postDataJSON())
  })
  await page.goto(`${app.baseUrl}/history.html?platform=${platform}`)
  const consent = page.getByRole('alertdialog').getByRole('button', { name: 'Disable' })
  await consent.waitFor({ timeout: 5000 }).catch(() => {})
  if (await consent.isVisible()) await consent.click()
  await writeFile(
    `${output}/startup.json`,
    JSON.stringify(
      {
        path: new URL(page.url()).pathname,
        headings: await page.getByRole('heading').allTextContents(),
        rows: await page.locator('[data-item-id]').count(),
      },
      null,
      2
    )
  )
  await page.locator(`[data-item-id="${entryId}"]`).click()
  const completed = page.getByTestId(`delivery-status-${peerIds[0]}`)
  const failed = page.getByTestId(`delivery-status-${peerIds[1]}`)
  await failed.waitFor()
  assert.equal(await completed.getAttribute('data-tone'), 'positive')
  assert.equal(await failed.getAttribute('data-tone'), 'critical')
  await failed.hover()
  const details = page.getByTestId('delivery-transfer-details')
  await details.getByText('delivery-real-file.bin', { exact: true }).waitFor()
  await page.screenshot({ path: `${output}/failed-peer.png` })
  await backend.control('restore-receiver')
  await failed.click()
  const confirm = page.getByTestId('resend-confirm-dialog')
  await confirm.waitFor()
  assert.equal(requests.length, 1)
  assert.deepEqual(requests[0], { entryId, peers: [peerIds[1]], confirmNewTransfer: false })
  await confirm.getByRole('button', { name: 'Cancel', exact: true }).click()
  assert.equal(await failed.getAttribute('data-tone'), 'critical')
  await failed.click()
  await confirm.waitFor()
  const retryStartedAt = performance.now()
  await confirm.getByRole('button', { name: 'Send again', exact: true }).click()
  const progress = await page.waitForFunction(id => {
    const bar = document.querySelector(`[data-testid="delivery-status-${id}"] [role="progressbar"]`)
    const value = bar?.getAttribute('aria-valuenow')
    return value != null && Number(value) < 100 ? Number(value) : false
  }, peerIds[1])
  const progressValue = await progress.jsonValue()
  await page.screenshot({ path: `${output}/retry-in-progress.png` })
  await page.waitForFunction(
    id =>
      document.querySelector(`[data-testid="delivery-status-${id}"]`)?.getAttribute('data-tone') ===
      'positive',
    peerIds[1],
    // Match the fixture's real-transfer budget for the 64 MiB payload.
    { timeout: 90_000 }
  )
  const retryDurationMs = Math.round(performance.now() - retryStartedAt)
  assert.equal(await completed.getAttribute('data-tone'), 'positive')
  assert.equal(await completed.getByRole('progressbar').count(), 0)
  assert.equal(await failed.getByRole('progressbar').count(), 0)
  assert.deepEqual(requests, [
    { entryId, peers: [peerIds[1]], confirmNewTransfer: false },
    { entryId, peers: [peerIds[1]], confirmNewTransfer: false },
    { entryId, peers: [peerIds[1]], confirmNewTransfer: true },
  ])
  const verified = await backend.control('verify-files')
  assert.equal(verified.filesVerified, 2)
  await page.screenshot({ path: `${output}/retried-peer.png` })
  await writeFile(
    `${output}/delivery-live.json`,
    JSON.stringify(
      {
        passed: true,
        progressValue,
        retryDurationMs,
        verified,
        entryId,
        peerIds,
        requests,
        assertions: [
          'real-failed-snapshot',
          'peer-isolation',
          'cancel-keeps-failed',
          'confirmed-retry-target',
          'live-completion-without-reload',
        ],
      },
      null,
      2
    ) + '\n'
  )
})
