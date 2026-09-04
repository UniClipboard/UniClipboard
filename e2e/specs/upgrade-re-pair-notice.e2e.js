import fs from 'node:fs'
import path from 'node:path'

const enabled = process.env.E2E_UPGRADE_REPAIR === '1'
const expectCleared = process.env.E2E_UPGRADE_REPAIR_CLEARED === '1'
const passphrase = process.env.E2E_UPGRADE_PASSPHRASE ?? 'hunter22hunter22'
const screenshotDir = process.env.E2E_SCREENSHOT_DIR ?? path.resolve('e2e', 'artifacts')

async function finishAnimations() {
  await browser.execute(() => {
    for (const animation of document.getAnimations()) {
      try {
        animation.finish()
      } catch {
        // WebKit can expose a completed transition that no longer accepts finish().
      }
    }
  })
}

describe('historical upgrade re-pair notice', () => {
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

    const rePairingNotice = await $('[data-slot="alert-dialog-content"]')
    await rePairingNotice.waitForDisplayed({
      timeout: 60000,
      timeoutMsg: 'visible re-pair notice did not appear after unlock',
    })
    expect(await rePairingNotice.getText()).toContain('请重新配对设备')
    await finishAnimations()
    await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-notice.png'))

    const buttons = await $$('button')
    let openDevicesButton = null
    for (const button of buttons) {
      if ((await button.isDisplayed()) && (await button.getText()).includes('前往设备管理')) {
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
    await browser.pause(1000)
    if (!(await browser.getUrl()).endsWith('/devices')) {
      const openedFromSidebar = await browser.execute(() => {
        const link = document.querySelector('a[href="/devices"]')
        if (!(link instanceof HTMLElement)) return false
        link.click()
        return true
      })
      expect(openedFromSidebar).toBe(true)
    }
    await finishAnimations()
    const addDeviceButton = await $('[data-testid="devices-add-device"]')
    await addDeviceButton.waitForExist({
      timeout: 30000,
      timeoutMsg: 'device management page did not finish rendering',
    })
    await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-devices.png'))

    const opened = await browser.execute(() => {
      const button = Array.from(
        document.querySelectorAll('[data-testid="devices-add-device"]')
      ).find(candidate => candidate instanceof HTMLElement && candidate.offsetParent !== null)
      if (!(button instanceof HTMLElement)) return false
      button.click()
      return true
    })
    expect(opened).toBe(true)
    const passphraseStep = await $('[data-testid="re-pairing-passphrase-step"]')
    await passphraseStep.waitForExist({
      timeout: 30000,
      timeoutMsg: 're-pairing invitation did not request the original passphrase',
    })
    await finishAnimations()
    const setPassphrase = async value => {
      const changed = await browser.execute(nextValue => {
        const input = document.querySelector('#re-pairing-passphrase')
        if (!(input instanceof HTMLInputElement)) return false
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
        setter?.call(input, nextValue)
        input.dispatchEvent(new Event('input', { bubbles: true }))
        input.dispatchEvent(new Event('change', { bubbles: true }))
        return true
      }, value)
      expect(changed).toBe(true)
    }
    const confirmPassphrase = async () => {
      await browser.waitUntil(
        async () =>
          browser.execute(() => {
            const button = document.querySelector('[data-testid="re-pairing-confirm-passphrase"]')
            return button instanceof HTMLButtonElement && !button.disabled
          }),
        { timeout: 10000, timeoutMsg: 'passphrase confirmation did not become available' }
      )
      const clicked = await browser.execute(() => {
        const button = document.querySelector('[data-testid="re-pairing-confirm-passphrase"]')
        if (!(button instanceof HTMLElement)) return false
        button.click()
        return true
      })
      expect(clicked).toBe(true)
    }

    await setPassphrase(`${passphrase}-wrong`)
    await confirmPassphrase()
    await passphraseStep.waitForExist({
      timeout: 30000,
      timeoutMsg: 'wrong passphrase unexpectedly left the confirmation step',
    })
    expect(await $('[data-testid="add-device-invitation-code"]').isExisting()).toBe(false)
    await finishAnimations()
    await browser.pause(500)
    await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-wrong-passphrase.png'))

    await setPassphrase(passphrase)
    await confirmPassphrase()
    await $('[data-testid="add-device-invitation-code"]').waitForExist({
      timeout: 30000,
      timeoutMsg: 'correct original passphrase did not produce an invitation',
    })
    await finishAnimations()
    await browser.pause(500)
    await browser.saveScreenshot(path.join(screenshotDir, 'upgrade-re-pair-invitation.png'))
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
