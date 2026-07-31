import { browser, expect } from '@wdio/globals'

const dualDescribe = browser.isMultiremote ? describe : describe.skip

async function element(instance, selector, { timeout = 30000 } = {}) {
  const target = await instance.$(selector)
  await target.waitForDisplayed({ timeout })
  return target
}

async function click(instance, selector) {
  const target = await element(instance, selector)
  await instance.execute(button => button.click(), target)
}

async function invitationCode(instance) {
  const display = await element(instance, '[data-testid="setup-invitation-code"]', {
    timeout: 60000,
  })
  const code = (await display.getText()).replace(/[^A-Z0-9]/g, '')
  expect(code).toHaveLength(8)
  return code
}

async function enterInvitation(instance, code, passphrase) {
  const codeInput = await element(instance, '#join-code')
  await codeInput.setValue(code)
  expect(await codeInput.getValue()).toBe(code)
  const passphraseInput = await element(instance, '#join-pass')
  expect(await passphraseInput.getValue()).toBe('')
  await passphraseInput.setValue(passphrase)
}

dualDescribe('同机双客户端首次配对', () => {
  it('拒绝失效邀请码和错误口令后仍可用全新邀请码完成加入', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-dual-peer-passphrase'

    await Promise.all([sponsor.tauri.switchWindow('main'), joiner.tauri.switchWindow('main')])

    await Promise.all([
      element(sponsor, '[data-testid="setup-entry-create"]', { timeout: 60000 }),
      element(joiner, '[data-testid="setup-entry-join"]', { timeout: 60000 }),
    ])

    await click(sponsor, '[data-testid="setup-entry-create"]')
    await (await element(sponsor, '#device-name')).setValue('E2E Sponsor')
    await (await element(sponsor, '#pass1')).setValue(passphrase)
    await (await element(sponsor, '#pass2')).setValue(passphrase)
    await click(sponsor, '[data-testid="setup-initialize-submit"]')

    await click(sponsor, '[data-testid="setup-complete-invite"]')
    const canceledCode = await invitationCode(sponsor)

    await click(sponsor, '[data-testid="setup-invitation-cancel"]')
    await element(sponsor, '[data-testid="setup-complete-invite"]')

    await click(joiner, '[data-testid="setup-entry-join"]')
    await enterInvitation(joiner, canceledCode, passphrase)
    await click(joiner, '[data-testid="setup-redeem-submit"]')
    await element(joiner, '[role="alert"]')
    expect(await (await element(joiner, '#join-code')).getValue()).toBe('')

    await click(sponsor, '[data-testid="setup-complete-invite"]')
    const activeCode = await invitationCode(sponsor)
    expect(activeCode).not.toBe(canceledCode)

    await enterInvitation(joiner, activeCode, 'wrong-e2e-passphrase')
    await click(joiner, '[data-testid="setup-redeem-submit"]')
    await element(joiner, '[role="alert"]')
    expect(await (await element(joiner, '#join-code')).getValue()).toBe('')

    await click(sponsor, '[data-testid="setup-invitation-cancel"]')
    await element(sponsor, '[data-testid="setup-complete-invite"]')
    await click(sponsor, '[data-testid="setup-complete-invite"]')
    const finalCode = await invitationCode(sponsor)
    expect(finalCode).not.toBe(activeCode)

    await enterInvitation(joiner, finalCode, passphrase)
    await click(joiner, '[data-testid="setup-redeem-submit"]')

    const [sponsorComplete, joinerComplete] = await Promise.all([
      element(sponsor, '[data-testid="setup-pairing-complete"]', { timeout: 90000 }),
      element(joiner, '[data-testid="setup-pairing-complete"]', { timeout: 90000 }),
    ])
    await expect(sponsorComplete).toExist()
    await expect(joinerComplete).toExist()

    const [sponsorPeer, joinerPeer] = await Promise.all([
      (await element(sponsor, '[data-testid="setup-complete-peer-id"]')).getText(),
      (await element(joiner, '[data-testid="setup-complete-peer-id"]')).getText(),
    ])

    expect(sponsorPeer).not.toBe('---')
    expect(joinerPeer).not.toBe('---')
    expect(sponsorPeer).not.toBe(joinerPeer)
  })
})
