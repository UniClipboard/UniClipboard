import { browser, expect } from '@wdio/globals'
import {
  click,
  daemonConnection,
  daemonRequest,
  dualDescribe,
  element,
  pairFreshProfiles,
  unlockPeer,
  waitForPairedPeer,
} from '../helpers/dualPeer.js'

dualDescribe('文字同步与历史操作', () => {
  it('接收文字后无需刷新即可在历史中打开', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-text-history-passphrase'
    const text = `gui-e2e-text-${Date.now()}`
    await pairFreshProfiles({ sponsor, joiner, passphrase })

    const sponsorConnection = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE)
    const joinerConnection = daemonConnection(process.env.E2E_UC_JOINER_PROFILE)
    await Promise.all([
      click(sponsor, '[data-testid="setup-complete-done"]'),
      click(joiner, '[data-testid="setup-complete-done"]'),
    ])
    await Promise.all([
      element(sponsor, '[data-testid="history-preview-motion"]'),
      element(joiner, '[data-testid="history-preview-motion"]'),
    ])
    await Promise.all([
      unlockPeer(sponsor, sponsorConnection, passphrase),
      unlockPeer(joiner, joinerConnection, passphrase),
    ])
    await Promise.all([
      waitForPairedPeer(sponsor, sponsorConnection),
      waitForPairedPeer(joiner, joinerConnection),
    ])

    const dispatchResponse = await daemonRequest(sponsorConnection, '/clipboard/dispatch', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text }),
    })
    expect(dispatchResponse.status).toBe(200)

    await expect(
      await element(joiner, '[data-testid="clipboard-detail"]', { timeout: 30000 })
    ).toHaveText(expect.stringContaining(text))
  })
})
