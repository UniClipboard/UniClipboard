import fs from 'node:fs'
import path from 'node:path'
import { browser, expect } from '@wdio/globals'
import {
  click,
  daemonConnection,
  daemonRequest,
  dualDescribe,
  element,
  enterInvitation,
  pageDiagnostics,
  setupEntry,
  showMainWindow,
} from '../helpers/dualPeer.js'

const passphrase = process.env.E2E_UPGRADE_PASSPHRASE ?? 'upgrade-fixture-passphrase'
const screenshotDir = process.env.E2E_SCREENSHOT_DIR ?? path.resolve('e2e', 'artifacts')

async function openUpgradedSponsor(sponsor) {
  const notice = await sponsor.$('[data-slot="alert-dialog-content"]')
  const unlockButton = await sponsor.$('button*=解锁')
  try {
    await sponsor.waitUntil(
      async () => (await notice.isDisplayed()) || (await unlockButton.isDisplayed()),
      { timeout: 60000, timeoutMsg: 'upgraded sponsor did not reach unlock or recovery notice' }
    )
  } catch (error) {
    fs.mkdirSync(screenshotDir, { recursive: true })
    await sponsor.saveScreenshot(path.join(screenshotDir, 'sponsor-initial-state-failure.png'))
    console.error('Upgraded sponsor initial diagnostics:', {
      url: await sponsor.getUrl(),
      ...(await pageDiagnostics(sponsor)),
      dialogs: await sponsor.execute(() =>
        Array.from(document.querySelectorAll('[data-slot="alert-dialog-content"]')).map(node => {
          const style = getComputedStyle(node)
          const rect = node.getBoundingClientRect()
          return {
            attributes: Object.fromEntries(
              Array.from(node.attributes, item => [item.name, item.value])
            ),
            display: style.display,
            visibility: style.visibility,
            opacity: style.opacity,
            transform: style.transform,
            width: rect.width,
            height: rect.height,
          }
        })
      ),
    })
    throw error
  }
  if (await unlockButton.isDisplayed()) {
    await unlockButton.click()
    const input = await sponsor.$('#unlock-passphrase')
    await sponsor.waitUntil(
      async () => (await notice.isDisplayed()) || (await input.isDisplayed()),
      { timeout: 30000, timeoutMsg: 'upgraded sponsor did not request its passphrase' }
    )
    if (await input.isDisplayed()) {
      await input.setValue(passphrase)
      await sponsor.keys('Enter')
    }
  }
  await notice.waitForDisplayed({ timeout: 60000 })
  await click(sponsor, '[data-slot="alert-dialog-action"]')
  await element(sponsor, '[data-testid="devices-add-device"]', { timeout: 30000 })
}

async function issueRecoveryInvitation(sponsor) {
  await click(sponsor, '[data-testid="devices-add-device"]')
  await element(sponsor, '[data-testid="re-pairing-passphrase-step"]')
  await (await element(sponsor, '#re-pairing-passphrase')).setValue(passphrase)
  const confirm = await element(sponsor, '[data-testid="re-pairing-confirm-passphrase"]')
  await confirm.waitForEnabled({ timeout: 10000 })
  await confirm.click()
  const codeDisplay = await element(sponsor, '[data-testid="add-device-invitation-code"]', {
    timeout: 30000,
  })
  const code = (await codeDisplay.getText()).replace(/[^A-Z0-9]/g, '')
  expect(code).toHaveLength(8)
  return code
}

function observeElement(instance, selector) {
  return instance.executeAsync((targetSelector, done) => {
    let settled = false
    const finish = result => {
      if (settled) return
      settled = true
      observer.disconnect()
      clearTimeout(timeout)
      done(result)
    }
    const inspect = () => {
      const node = document.querySelector(targetSelector)
      if (!(node instanceof HTMLElement)) return
      const style = getComputedStyle(node)
      const rect = node.getBoundingClientRect()
      finish({
        found: true,
        displayed:
          style.display !== 'none' &&
          style.visibility !== 'hidden' &&
          Number(style.opacity) > 0 &&
          rect.width > 0 &&
          rect.height > 0,
        observedAt: Date.now(),
      })
    }
    const observer = new MutationObserver(inspect)
    observer.observe(document.documentElement, { childList: true, subtree: true })
    const timeout = setTimeout(
      () => finish({ found: false, displayed: false, observedAt: Date.now() }),
      30000
    )
    inspect()
  }, selector)
}

dualDescribe('历史版本升级后重新配对', () => {
  it('用户重新配对后两个窗口立即显示成功并清除恢复状态', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    await Promise.all([sponsor.tauri.switchWindow('main'), joiner.tauri.switchWindow('main')])
    await Promise.all([showMainWindow(sponsor), showMainWindow(joiner)])
    await setupEntry(joiner, '[data-testid="setup-entry-join"]', 'Joiner')
    await openUpgradedSponsor(sponsor)
    const code = await issueRecoveryInvitation(sponsor)

    await click(joiner, '[data-testid="setup-entry-join"]')
    await enterInvitation(joiner, code, passphrase)
    const startedAt = Date.now()
    const sponsorSuccess = observeElement(sponsor, '[data-testid="add-device-success"]')
    const joinerSuccess = observeElement(joiner, '[data-testid="setup-pairing-complete"]')
    await click(joiner, '[data-testid="setup-redeem-submit"]')

    const sponsorResult = await sponsorSuccess
    expect(sponsorResult.found).toBe(true)
    expect(sponsorResult.displayed).toBe(true)

    const joinerResult = await joinerSuccess
    expect(joinerResult.found).toBe(true)
    expect(joinerResult.displayed).toBe(true)
    expect(Math.max(sponsorResult.observedAt, joinerResult.observedAt) - startedAt).toBeLessThan(
      30_000
    )

    const connection = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE)
    const response = await daemonRequest(connection, '/v2/setup/state')
    expect(response.status).toBe(200)
    expect((await response.json()).data.rePairingRequired).toBe(false)
  })
})
