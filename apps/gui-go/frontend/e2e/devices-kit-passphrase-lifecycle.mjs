import assert from 'node:assert/strict'

// Synthetic transport responses exercise the complete app's UI lifetime without changing keys.
// This is not real-daemon passphrase or physical-peer acceptance. Never record request bodies.
export async function passphraseLifecycle(driver, outcome, detachOwner = false) {
  const { page, sheet, option, openSheet, closeSheet } = driver
  let signalPending
  const pending = new Promise(resolve => {
    signalPending = resolve
  })
  let releaseResponse
  const held = new Promise(resolve => {
    releaseResponse = resolve
  })
  let issues = 0
  const setupPattern = '**/v2/setup/state'
  const changePattern = '**/encryption/passphrase'
  const issuePattern = '**/v2/setup/issue-invitation'
  await page.route(setupPattern, async route => {
    const response = await route.fetch()
    const body = await response.json()
    const state = body.data ?? body
    state.currentInvitation = null
    state.rePairingRequired = true
    await route.fulfill({ response, json: body })
  })
  await page.route(changePattern, async route => {
    signalPending()
    await held
    await route.fulfill({
      status: outcome === 'success' ? 200 : 500,
      json:
        outcome === 'success'
          ? { data: { success: true }, ts: Date.now() }
          : { code: 'INTERNAL', message: 'Injected lifecycle failure' },
    })
  })
  await page.route(issuePattern, async route => {
    issues += 1
    await route.fulfill({
      json: { data: { code: '123456', expiresAtMs: Date.now() + 300000 }, ts: Date.now() },
    })
  })
  try {
    await openSheet()
    await sheet().getByRole('button', { name: 'Add', exact: true }).click()
    await page.getByRole('menuitem', { name: 'Invite Device…' }).click()
    await sheet().getByRole('button', { name: 'Forgot the old passphrase? Reset it' }).click()
    await page.locator('#new-space-passphrase').fill('Disposable-new-passphrase-42')
    await page.locator('#confirm-space-passphrase').fill('Disposable-new-passphrase-42')
    await sheet().getByRole('button', { name: 'Reset passphrase', exact: true }).click()
    let timer
    try {
      await Promise.race([
        pending,
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error('passphrase request was not sent')), 10000)
        }),
      ])
    } finally {
      clearTimeout(timer)
    }
    assert.equal(
      await sheet().getByRole('button', { name: 'Close', exact: true }).isDisabled(),
      true
    )
    await option('kit-browser').click()
    await sheet().getByTestId('device-invitation-row').click()
    assert.equal(await page.locator('#new-space-passphrase').isDisabled(), true)
    await closeSheet()
    await sheet().waitFor({ state: 'detached' })
    await openSheet()
    await page.keyboard.press('Escape')
    await sheet().waitFor({ state: 'detached' })
    await openSheet()
    assert.equal(await page.locator('#new-space-passphrase').isDisabled(), true)
    await page.mouse.click(1, page.viewportSize().height - 1)
    // Backdrop dismissal differs by skin; either outcome must preserve the pending controller.
    const backdropDismissed = !(await sheet().isVisible())
    if (backdropDismissed) {
      await sheet().waitFor({ state: 'detached' })
      await openSheet()
    }
    assert.equal(await page.locator('#new-space-passphrase').isDisabled(), true)
    if (detachOwner) {
      await page.evaluate(() => {
        window.history.pushState({}, '', '/settings')
        window.dispatchEvent(new PopStateEvent('popstate'))
      })
      await sheet().waitFor({ state: 'detached' })
    } else {
      await closeSheet()
      await sheet().waitFor({ state: 'detached' })
    }
    const settled = page.waitForResponse(response =>
      new URL(response.url()).pathname.endsWith('/encryption/passphrase')
    )
    releaseResponse()
    await settled
    if (detachOwner) {
      await page
        .getByText(
          outcome === 'success'
            ? 'Space passphrase reset'
            : 'Could not reset the passphrase. Please try again.',
          { exact: true }
        )
        .waitFor()
      assert.equal(issues, 0, 'a detached owner must not issue an invitation')
    } else {
      await openSheet()
      if (outcome === 'failure') {
        await sheet().getByRole('alert').waitFor()
        assert.equal(
          await page.locator('#new-space-passphrase').inputValue(),
          'Disposable-new-passphrase-42'
        )
        assert.equal(issues, 0)
        await sheet().getByRole('button', { name: 'Cancel', exact: true }).click()
        await sheet().getByRole('button', { name: 'Forgot the old passphrase? Reset it' }).click()
        assert.equal(await page.locator('#new-space-passphrase').inputValue(), '')
        assert.equal(
          await sheet().getByRole('alert').count(),
          0,
          'Cancel clears the prior draft and error'
        )
      } else {
        await sheet()
          .getByTestId('add-device-invitation-code')
          .filter({ hasText: '123-456' })
          .waitFor()
        assert.equal(issues, 1, 'the active row owns the follow-up invitation')
      }
    }
    return {
      outcome,
      detachOwner,
      issues,
      backdropDismissed,
      responseMode: 'synthetic-ui-lifecycle',
    }
  } finally {
    releaseResponse()
    await page.unroute(setupPattern)
    await page.unroute(changePattern)
    await page.unroute(issuePattern)
  }
}
