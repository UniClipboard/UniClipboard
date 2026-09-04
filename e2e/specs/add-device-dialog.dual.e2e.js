import { browser, expect } from '@wdio/globals'
import {
  click,
  dualDescribe,
  element,
  enterInvitation,
  initializeSponsor,
  openFreshSetup,
  pairingComplete,
} from '../helpers/dualPeer.js'

dualDescribe('设备页添加设备', () => {
  it('新设备加入后弹窗和加入窗口都会显示成功', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-add-device-dialog-passphrase'

    await openFreshSetup(sponsor, joiner)
    await initializeSponsor(sponsor, passphrase)
    await click(sponsor, '[data-testid="setup-complete-later"]')
    await element(sponsor, '[data-testid="history-preview-motion"]')
    await click(sponsor, 'a[href="/devices"]')
    await click(sponsor, '[data-testid="devices-add-device"]')

    const codeDisplay = await element(sponsor, '[data-testid="add-device-invitation-code"]', {
      timeout: 60000,
    })
    const code = (await codeDisplay.getText()).replace(/[^A-Z0-9]/g, '')
    expect(code).toHaveLength(8)

    await click(joiner, '[data-testid="setup-entry-join"]')
    await enterInvitation(joiner, code, passphrase)
    const startedAt = Date.now()
    await click(joiner, '[data-testid="setup-redeem-submit"]')

    await expect(
      await element(sponsor, '[data-testid="add-device-success"]', { timeout: 30000 })
    ).toExist()
    await expect(await pairingComplete(joiner, 'Joiner')).toExist()
    expect(Date.now() - startedAt).toBeLessThan(30_000)
  })
})
