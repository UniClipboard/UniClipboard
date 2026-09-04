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
  pairingComplete,
  waitForDaemonUnreachable,
  waitForDaemonReplacement,
} from '../helpers/dualPeer.js'

dualDescribe('等待中的加入恢复', () => {
  it('加入方后台重启后继续同一任务并完成配对', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-join-restart-passphrase'

    await openFreshSetup(sponsor, joiner)
    await initializeSponsor(sponsor, passphrase)
    const code = await issueInvitation(sponsor)
    const sponsorDaemon = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE)
    const joinerDaemon = daemonConnection(process.env.E2E_UC_JOINER_PROFILE)

    process.kill(sponsorDaemon.pid, 'SIGSTOP')
    try {
      await click(joiner, '[data-testid="setup-entry-join"]')
      await enterInvitation(joiner, code, passphrase)
      await click(joiner, '[data-testid="setup-redeem-submit"]')
      await expect(
        await element(joiner, '[data-testid="setup-join-pending"]', { timeout: 15000 })
      ).toExist()

      const restartTriggered = await joiner.execute(() => {
        void window.__TAURI_INTERNALS__.invoke('restart_daemon', { trace: null })
        return true
      })
      expect(restartTriggered).toBe(true)
      await waitForDaemonUnreachable(joiner, joinerDaemon)
      process.kill(sponsorDaemon.pid, 'SIGCONT')
      await waitForDaemonReplacement(joiner, process.env.E2E_UC_JOINER_PROFILE, joinerDaemon.pid)

      const [sponsorComplete, joinerComplete] = await Promise.all([
        pairingComplete(sponsor, 'Sponsor'),
        pairingComplete(joiner, 'Joiner'),
      ])
      await expect(sponsorComplete).toExist()
      await expect(joinerComplete).toExist()
    } finally {
      try {
        process.kill(sponsorDaemon.pid, 'SIGCONT')
      } catch {
        // The app may already have replaced the suspended daemon.
      }
    }
  })
})
