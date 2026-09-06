import { browser, expect } from '@wdio/globals'
import {
  click,
  daemonConnection,
  daemonRequest,
  dualDescribe,
  element,
  enterInvitation,
  initializeSponsor,
  issueInvitation,
  openFreshSetup,
  pairingComplete,
  pageDiagnostics,
  showMainWindow,
} from '../helpers/dualPeer.js'

async function pairedDevices(instance, connection, count) {
  let devices = []
  await instance.waitUntil(
    async () => {
      const response = await daemonRequest(connection, '/paired-devices')
      if (response.status !== 200) return false
      devices = (await response.json()).data
      return devices.length === count
    },
    { timeout: 30000, timeoutMsg: `expected ${count} paired devices` }
  )
  return devices
}

async function dialogClosed(instance, timeout = 5000) {
  try {
    await (
      await instance.$('[data-testid="device-trust-dialog"]')
    ).waitForExist({ timeout, reverse: true })
    return true
  } catch {
    return false
  }
}

async function acceptRemovalAsRetained(instance) {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    await click(instance, '[data-testid="device-trust-choice-apply"]')
    const confirm = await element(instance, '[data-testid="device-trust-confirm"]')
    await confirm.waitForEnabled({ timeout: 10000 })
    await confirm.click()
    await instance.waitUntil(
      async () =>
        !(await (await instance.$('[data-testid="device-trust-dialog"]')).isExisting()) ||
        (await (await instance.$('[data-testid="device-trust-error"]')).isExisting()),
      { timeout: 10000, timeoutMsg: 'retained choice produced no visible outcome' }
    )
    if (await dialogClosed(instance, 500)) return 'completed'
    const error = await instance.$('[data-testid="device-trust-error"]')
    if ((await error.getAttribute('data-error')) === 'choice_pending') return 'pending'
  }
  throw new Error(
    `retained device choice did not settle: ${JSON.stringify(await pageDiagnostics(instance))}`
  )
}

async function confirmLocalRemoval(instance) {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    await click(instance, '[data-testid="device-trust-choice-apply"]')
    const confirm = await element(instance, '[data-testid="device-trust-confirm"]')
    await confirm.waitForEnabled({ timeout: 10000 })
    await confirm.click()
    try {
      await element(instance, '[data-testid="device-trust-local-removal-warning"]', {
        timeout: 5000,
      })
      const confirmExit = await element(instance, '[data-testid="device-trust-confirm"]')
      await confirmExit.waitForEnabled({ timeout: 10000 })
      await confirmExit.click()
      if (await dialogClosed(instance)) return
    } catch {
      if (await dialogClosed(instance, 500)) return
    }
  }
  throw new Error(
    `local removal choice did not settle: ${JSON.stringify(await pageDiagnostics(instance))}`
  )
}

dualDescribe('三设备离线驱逐与设备组选择', () => {
  it('保留设备接受变化且离线设备恢复后确认退出', async () => {
    const sponsor = browser.sponsor
    const retained = browser.retained
    const removed = browser.removed
    const passphrase = 'e2e-three-device-removal-passphrase'

    await openFreshSetup(sponsor, retained)
    await removed.tauri.switchWindow('main')
    await showMainWindow(removed)
    await element(removed, '[data-testid="setup-entry-join"]', { timeout: 60000 })

    await initializeSponsor(sponsor, passphrase, 'E2E Sponsor')
    const retainedCode = await issueInvitation(sponsor)
    await click(retained, '[data-testid="setup-entry-join"]')
    await enterInvitation(retained, retainedCode, passphrase)
    await click(retained, '[data-testid="setup-redeem-submit"]')
    await Promise.all([pairingComplete(sponsor, 'Sponsor'), pairingComplete(retained, 'Retained')])
    await Promise.all([
      click(sponsor, '[data-testid="setup-complete-done"]'),
      click(retained, '[data-testid="setup-complete-done"]'),
    ])
    await Promise.all([
      element(sponsor, '[data-testid="history-preview-motion"]'),
      element(retained, '[data-testid="history-preview-motion"]'),
    ])

    const sponsorConnection = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE)
    const retainedDevices = await pairedDevices(sponsor, sponsorConnection, 1)
    const retainedId = retainedDevices[0].peerId
    await click(sponsor, 'a[href="/devices"]')
    await click(sponsor, '[data-testid="devices-add-device"]')
    const removedCodeDisplay = await element(
      sponsor,
      '[data-testid="add-device-invitation-code"]',
      { timeout: 60000 }
    )
    const removedCode = (await removedCodeDisplay.getText()).replace(/[^A-Z0-9]/g, '')
    expect(removedCode).toHaveLength(8)
    await click(removed, '[data-testid="setup-entry-join"]')
    await enterInvitation(removed, removedCode, passphrase)
    await click(removed, '[data-testid="setup-redeem-submit"]')
    await Promise.all([
      element(sponsor, '[data-testid="add-device-success"]', { timeout: 30000 }),
      pairingComplete(removed, 'Removed'),
    ])
    await click(removed, '[data-testid="setup-complete-done"]')
    await element(removed, '[data-testid="history-preview-motion"]')

    const allDevices = await pairedDevices(sponsor, sponsorConnection, 2)
    const removedDevice = allDevices.find(device => device.peerId !== retainedId)
    expect(removedDevice).toBeDefined()
    await (
      await sponsor.$('[data-testid="add-device-success"]')
    ).waitForExist({ timeout: 10000, reverse: true })
    await Promise.all([click(retained, 'a[href="/devices"]'), click(removed, 'a[href="/devices"]')])

    const removedConnection = daemonConnection(process.env.E2E_UC_REMOVED_PROFILE)
    process.kill(removedConnection.pid, 'SIGSTOP')
    try {
      await click(sponsor, `[data-testid="device-peer-${removedDevice.peerId}"]`)
      await click(sponsor, '[data-testid="device-unpair"]')
      await click(sponsor, '[data-testid="device-unpair-confirm"]')

      try {
        await element(retained, '[data-testid="device-trust-dialog"]', { timeout: 30000 })
      } catch (error) {
        console.error('Retained device diagnostics:', await pageDiagnostics(retained))
        throw error
      }
      process.kill(removedConnection.pid, 'SIGCONT')
      try {
        await element(removed, '[data-testid="device-trust-dialog"]', { timeout: 30000 })
      } catch (error) {
        console.error('Removed device diagnostics:', await pageDiagnostics(removed))
        throw error
      }
      const retainedOutcome = await acceptRemovalAsRetained(retained)
      expect(['pending', 'completed']).toContain(retainedOutcome)
      await confirmLocalRemoval(removed)
      await (
        await retained.$('[data-testid="device-trust-dialog"]')
      ).waitForExist({ timeout: 30000, reverse: true })
      await expect(
        await element(removed, '[data-testid="device-local"][data-status="removed"]')
      ).toExist()
    } finally {
      try {
        process.kill(removedConnection.pid, 'SIGCONT')
      } catch {
        // The test runner may already have stopped the removed device.
      }
    }
  })
})
