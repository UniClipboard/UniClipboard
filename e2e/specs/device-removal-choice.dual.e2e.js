import { browser, expect } from '@wdio/globals'
import {
  click,
  dualDescribe,
  element,
  pairFreshProfiles,
  pageDiagnostics,
} from '../helpers/dualPeer.js'

dualDescribe('设备移除与设备组选择', () => {
  it('被移除设备二次确认退出后显示已移除状态', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-device-removal-passphrase'
    await pairFreshProfiles({ sponsor, joiner, passphrase })

    await Promise.all([
      click(sponsor, '[data-testid="setup-complete-done"]'),
      click(joiner, '[data-testid="setup-complete-done"]'),
    ])
    await Promise.all([
      element(sponsor, '[data-testid="history-preview-motion"]'),
      element(joiner, '[data-testid="history-preview-motion"]'),
    ])
    await Promise.all([click(sponsor, 'a[href="/devices"]'), click(joiner, 'a[href="/devices"]')])

    await click(sponsor, '[data-testid^="device-peer-"]')
    await click(sponsor, '[data-testid="device-unpair"]')
    await click(sponsor, '[data-testid="device-unpair-confirm"]')

    try {
      await element(joiner, '[data-testid="device-trust-dialog"]', { timeout: 30000 })
    } catch (error) {
      console.error('Joiner device-trust diagnostics:', await pageDiagnostics(joiner))
      throw error
    }
    await click(joiner, '[data-testid="device-trust-choice-apply"]')
    await click(joiner, '[data-testid="device-trust-confirm"]')
    await expect(
      await element(joiner, '[data-testid="device-trust-local-removal-warning"]')
    ).toExist()
    await click(joiner, '[data-testid="device-trust-confirm"]')
    await (
      await joiner.$('[data-testid="device-trust-dialog"]')
    ).waitForExist({ timeout: 30000, reverse: true })
    await expect(
      await element(joiner, '[data-testid="device-local"][data-status="removed"]')
    ).toExist()
  })
})
