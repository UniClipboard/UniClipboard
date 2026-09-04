import { browser, expect } from '@wdio/globals'
import {
  click,
  dualDescribe,
  element,
  enterInvitation,
  initializeSponsor,
  issueInvitation,
  openFreshSetup,
  pairingComplete,
} from '../helpers/dualPeer.js'

async function expectRejected(joiner) {
  return element(joiner, '[data-testid="setup-join-rejected"]', { timeout: 30000 })
}

async function retryJoin(joiner) {
  await click(joiner, '[data-testid="setup-join-rejected-back"]')
  await element(joiner, '#join-code')
}

dualDescribe('首次配对拒绝和重试', () => {
  it('失效邀请码和错误口令被拒绝后仍能用全新邀请码完成加入', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-pairing-rejection-passphrase'

    await openFreshSetup(sponsor, joiner)
    await initializeSponsor(sponsor, passphrase)
    const cancelledCode = await issueInvitation(sponsor)
    await click(sponsor, '[data-testid="setup-invitation-cancel"]')
    await element(sponsor, '[data-testid="setup-complete-invite"]')

    await click(joiner, '[data-testid="setup-entry-join"]')
    await enterInvitation(joiner, cancelledCode, passphrase)
    await click(joiner, '[data-testid="setup-redeem-submit"]')
    await expect(await expectRejected(joiner)).toExist()
    await retryJoin(joiner)

    const wrongPassphraseCode = await issueInvitation(sponsor)
    await enterInvitation(joiner, wrongPassphraseCode, 'wrong-e2e-passphrase')
    await click(joiner, '[data-testid="setup-redeem-submit"]')
    await expect(await expectRejected(joiner)).toExist()
    await retryJoin(joiner)

    await click(sponsor, '[data-testid="setup-invitation-cancel"]')
    await element(sponsor, '[data-testid="setup-complete-invite"]')
    const finalCode = await issueInvitation(sponsor)
    await enterInvitation(joiner, finalCode, passphrase)
    await click(joiner, '[data-testid="setup-redeem-submit"]')

    const [sponsorComplete, joinerComplete] = await Promise.all([
      pairingComplete(sponsor, 'Sponsor'),
      pairingComplete(joiner, 'Joiner'),
    ])
    await expect(sponsorComplete).toExist()
    await expect(joinerComplete).toExist()
  })
})
