import fs from 'node:fs'
import path from 'node:path'

const enabled = process.env.E2E_UPGRADE_REPAIR === '1'
const expectCleared = process.env.E2E_UPGRADE_REPAIR_CLEARED === '1'
const passphrase = process.env.E2E_UPGRADE_PASSPHRASE ?? 'hunter22hunter22'
const screenshotDir = process.env.E2E_SCREENSHOT_DIR ?? path.resolve('e2e', 'artifacts')

describe('v0.19.1 upgrade re-pair notice', () => {
  before(function () {
    if (!enabled) this.skip()
  })

  it('appears after unlock and opens device management', async () => {
    if (expectCleared) return
    await browser.tauri.switchWindow('main')

    fs.mkdirSync(screenshotDir, { recursive: true })
    await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-startup.png'))

    const unlockButton = await $('button*=解锁')
    await browser.waitUntil(
      async () =>
        (await unlockButton.isDisplayed()) ||
        (await $('body').getText()).includes('请重新配对设备'),
      { timeout: 30000, timeoutMsg: 'app did not reach unlock or re-pair notice' }
    )
    if (await unlockButton.isDisplayed()) {
      await unlockButton.click()
      await browser.pause(2000)
      await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-after-unlock.png'))

      const passphraseInput = await $('#unlock-passphrase')
      await browser.waitUntil(
        async () =>
          (await passphraseInput.isDisplayed()) ||
          (await $('body').getText()).includes('请重新配对设备'),
        { timeout: 30000, timeoutMsg: 'unlock did not open the app or request a passphrase' }
      )
      if (await passphraseInput.isDisplayed()) {
        await passphraseInput.setValue(passphrase)
        await browser.keys('Enter')
      }
    }

    await browser.waitUntil(async () => (await $('body').getText()).includes('请重新配对设备'), {
      timeout: 60000,
      timeoutMsg: 're-pair notice did not appear after unlock',
    })

    await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-notice.png'))

    const buttons = await $$('button')
    let openDevicesButton = null
    for (const button of buttons) {
      if ((await button.getText()).includes('前往设备管理')) {
        openDevicesButton = button
        break
      }
    }
    expect(openDevicesButton).not.toBeNull()
    await openDevicesButton.click()

    await browser.waitUntil(async () => (await browser.getUrl()).endsWith('/devices'), {
      timeout: 30000,
      timeoutMsg: 're-pair notice did not navigate to device management',
    })
    await browser.waitUntil(async () => await $('h2*=设备').isDisplayed(), {
      timeout: 30000,
      timeoutMsg: 'device management page did not finish rendering',
    })
    await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-devices.png'))
  })

  it('stays cleared after re-pairing and restart', async () => {
    if (!expectCleared) return

    await browser.tauri.switchWindow('main')
    fs.mkdirSync(screenshotDir, { recursive: true })

    const unlockButton = await $('button*=解锁')
    await unlockButton.waitForDisplayed({
      timeout: 30000,
      timeoutMsg: 'app did not reach unlock after restart',
    })
    await unlockButton.click()

    const passphraseInput = await $('#unlock-passphrase')
    await passphraseInput.waitForDisplayed({
      timeout: 30000,
      timeoutMsg: 'unlock did not request a passphrase',
    })
    await passphraseInput.setValue(passphrase)
    await browser.keys('Enter')

    await browser.waitUntil(async () => !(await passphraseInput.isDisplayed()), {
      timeout: 60000,
      timeoutMsg: 'app did not finish unlocking',
    })
    await browser.pause(2000)

    const bodyText = await $('body').getText()
    expect(bodyText).not.toContain('请重新配对设备')
    await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-cleared.png'))
  })
})
