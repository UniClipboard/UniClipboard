import assert from 'node:assert/strict'

// Call from the full-app browser driver with an isolated daemon. Never serialize credentials.
export async function credentialReopen({ page, sheet, option, openSheet, closeSheet }, branch) {
  assert.ok(['password', 'username'].includes(branch))
  const label = `lifecycle-${branch}`
  await sheet().getByRole('button', { name: 'Add', exact: true }).click()
  await page.getByRole('menuitem', { name: 'Connect Phone…' }).click()
  await sheet()
    .getByRole('button', { name: /^Regular sync/ })
    .click()
  const registration = page.getByRole('dialog').last()
  await registration.getByLabel('Device label', { exact: true }).fill(label)
  await registration.getByRole('button', { name: 'Add', exact: true }).click()
  const qr = () => sheet().locator('svg[aria-label="QR code that auto-fills the sync credentials"]')
  await qr().waitFor()
  await sheet().getByRole('button', { name: 'Done', exact: true }).click()
  await option(label).waitFor()
  // Completing the registration task discards its one-time credential before closing the sheet.
  // The fresh panel intentionally hides Edit. First verify the handoff expires on close.
  await closeSheet()
  await sheet().waitFor({ state: 'detached' })
  await openSheet()
  assert.equal(await qr().count(), 0, 'registration QR must not return after reopening')
  await sheet().getByRole('button', { name: 'Edit device', exact: true }).click()
  await page
    .locator(`#mobile-device-edit-${branch}`)
    .fill(branch === 'password' ? 'Disposable-rotation-42' : 'lifecycle_rotated')
  await sheet().getByRole('button', { name: 'Save', exact: true }).click()
  await page.locator('#mobile-device-edit-label').waitFor({ state: 'hidden' })
  await qr().waitFor()
  await closeSheet()
  await sheet().waitFor({ state: 'detached' })
  await openSheet()
  assert.equal(await option(label).getAttribute('aria-selected'), 'true')
  assert.equal(await qr().count(), 0, 'one-time QR must not return after reopening')
  await option('kit-browser').click()
  await option(label).click()
  assert.equal(await qr().count(), 0, 'one-time QR must not return after row navigation')
  return { branch, selectedIdentityRetained: true, staleQrAbsent: true }
}
