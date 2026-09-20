const passphrase = process.env.E2E_UNLOCK_PASSPHRASE

describe('content lock keyring and passphrase unlock', () => {
  it('uses the keyring when available and keeps the passphrase fallback usable', async () => {
    expect(passphrase).toBeTruthy()
    await browser.tauri.switchWindow('main')

    const unlockButton = await $('button*=解锁')
    await unlockButton.waitForDisplayed({
      timeout: 60000,
      timeoutMsg: 'app did not show the content lock',
    })

    await browser.tauri.switchWindow('quick-panel')
    await browser.waitUntil(async () => (await $('body').getText()).includes('解锁'), {
      timeout: 30000,
      timeoutMsg: 'quick panel did not respect the shared content lock',
    })
    expect(await $('input').isExisting()).toBe(false)
    await browser.tauri.switchWindow('main')

    await unlockButton.click()

    const passphraseInput = await $('#unlock-passphrase')
    await browser.waitUntil(
      async () =>
        (await passphraseInput.isDisplayed()) ||
        (await $('[data-testid="history-search-anchor"]').isDisplayed()) ||
        (await $('body').getText()).includes('请重新配对设备'),
      {
        timeout: 60000,
        timeoutMsg: 'keyring unlock neither opened the app nor requested the passphrase',
      }
    )

    if (await passphraseInput.isDisplayed()) {
      await passphraseInput.setValue(`${passphrase}-wrong`)
      await browser.keys('Enter')

      await browser.waitUntil(async () => (await $('body').getText()).includes('口令错误'), {
        timeout: 30000,
        timeoutMsg: `wrong passphrase response was not shown; body: ${await $('body').getText()}`,
      })
      await expect(passphraseInput).toBeDisplayed()

      await passphraseInput.setValue(passphrase)
      await browser.keys('Enter')
    }

    await browser.waitUntil(async () => !(await passphraseInput.isDisplayed()), {
      timeout: 60000,
      timeoutMsg: 'content lock did not close after authentication',
    })
    const dismissUpgradeNotice = await $('button*=不再提示')
    await browser.waitUntil(
      async () =>
        (await dismissUpgradeNotice.isDisplayed()) ||
        (await $('[data-testid="history-search-anchor"]').isDisplayed()),
      { timeout: 60000 }
    )
    if (await dismissUpgradeNotice.isDisplayed()) {
      await dismissUpgradeNotice.click()
      await $('[data-slot="alert-dialog-content"]').waitForExist({ reverse: true })
    }
    await $('[data-testid="history-search-anchor"]').waitForDisplayed({
      timeout: 60000,
      timeoutMsg: 'history did not become available after manual unlock',
    })
    await $('[data-testid="history-search-anchor"] button').click()
    await $('[data-testid="history-search-surface"] input').waitForDisplayed()
    await $('[data-testid="history-search-surface"] input').setValue('unlock-verification')
    await browser.execute(() => {
      for (const animation of document.getAnimations())
        if (animation.effect?.getComputedTiming().iterations !== Infinity) animation.finish()
    })
    await browser.saveScreenshot('/tmp/uniclipboard-content-unlocked.png')

    await browser.tauri.switchWindow('quick-panel')
    await $('input').waitForExist({ timeout: 30000 })
    await browser.tauri.switchWindow('main')
  })
})
