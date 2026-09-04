import { browser, expect } from '@wdio/globals'
import {
  click,
  dualDescribe,
  element,
  initializeSponsor,
  openFreshSetup,
} from '../helpers/dualPeer.js'

dualDescribe('切换空间', () => {
  it('已建立空间的设备从设备页加入另一个空间', async () => {
    const sponsor = browser.sponsor
    const switcher = browser.joiner
    const passphrase = 'e2e-switch-space-passphrase'

    await openFreshSetup(sponsor, switcher)
    await initializeSponsor(sponsor, passphrase, 'E2E Target Space')
    await initializeSponsor(switcher, passphrase, 'E2E Switcher')
    await Promise.all([
      click(sponsor, '[data-testid="setup-complete-later"]'),
      click(switcher, '[data-testid="setup-complete-later"]'),
    ])
    await Promise.all([
      element(sponsor, '[data-testid="history-preview-motion"]'),
      element(switcher, '[data-testid="history-preview-motion"]'),
    ])
    await Promise.all([click(sponsor, 'a[href="/devices"]'), click(switcher, 'a[href="/devices"]')])

    await click(sponsor, '[data-testid="devices-add-device"]')
    const codeDisplay = await element(sponsor, '[data-testid="add-device-invitation-code"]', {
      timeout: 60000,
    })
    const code = (await codeDisplay.getText()).replace(/[^A-Z0-9]/g, '')
    expect(code).toHaveLength(8)

    await click(switcher, '[data-testid="device-switch-space"]')
    await expect(await element(switcher, '[data-testid="switch-space-dialog"]')).toExist()
    await (await element(switcher, '#switch-code')).setValue(code)
    await (await element(switcher, '#switch-pass')).setValue(passphrase)
    const startedAt = Date.now()
    await click(switcher, '[data-testid="switch-space-submit"]')

    await expect(
      await element(sponsor, '[data-testid="add-device-success"]', { timeout: 30000 })
    ).toExist()
    await expect(
      await element(switcher, '[data-testid="switch-space-success"]', { timeout: 30000 })
    ).toExist()
    expect(Date.now() - startedAt).toBeLessThan(30_000)
  })
})
