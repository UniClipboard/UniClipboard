import { browser, expect } from '@wdio/globals'
import { dualDescribe, element, pairFreshProfiles } from '../helpers/dualPeer.js'

dualDescribe('首次配对成功反馈', () => {
  it('完成有效邀请后两个窗口都会在 30 秒内显示配对成功', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const { elapsedMs } = await pairFreshProfiles({
      sponsor,
      joiner,
      passphrase: 'e2e-pairing-feedback-passphrase',
    })

    expect(elapsedMs).toBeLessThan(30_000)
    const [sponsorPeer, joinerPeer] = await Promise.all([
      (await element(sponsor, '[data-testid="setup-complete-peer-id"]')).getText(),
      (await element(joiner, '[data-testid="setup-complete-peer-id"]')).getText(),
    ])
    expect(sponsorPeer).not.toBe('---')
    expect(joinerPeer).not.toBe('---')
  })
})
