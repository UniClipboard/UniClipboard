import { browser, expect } from '@wdio/globals'
import {
  click,
  daemonConnection,
  dualDescribe,
  element,
  enterInvitation,
  initializeSponsor,
  issueInvitation,
  openFreshSetup,
} from '../helpers/dualPeer.js'

dualDescribe('等待中的加入取消', () => {
  it('对方暂时不可用时可以取消，并在连接恢复后显示取消结果', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-join-cancel-passphrase'

    await openFreshSetup(sponsor, joiner)
    await initializeSponsor(sponsor, passphrase)
    const code = await issueInvitation(sponsor)
    const sponsorDaemon = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE)

    process.kill(sponsorDaemon.pid, 'SIGSTOP')
    try {
      await click(joiner, '[data-testid="setup-entry-join"]')
      await enterInvitation(joiner, code, passphrase)
      await click(joiner, '[data-testid="setup-redeem-submit"]')
      await expect(
        await element(joiner, '[data-testid="setup-join-pending"]', { timeout: 15000 })
      ).toExist()
      await click(joiner, '[data-testid="setup-join-cancel"]')

      process.kill(sponsorDaemon.pid, 'SIGCONT')
      await expect(
        await element(joiner, '[data-testid="setup-join-rejected"]', { timeout: 30000 })
      ).toExist()
    } finally {
      try {
        process.kill(sponsorDaemon.pid, 'SIGCONT')
      } catch {
        // The app may already have replaced the suspended daemon.
      }
    }
  })
})
