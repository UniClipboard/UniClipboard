import fs from 'node:fs'
import path from 'node:path'
import { browser, expect } from '@wdio/globals'
import {
  click,
  daemonConnection,
  daemonRequest,
  dualDescribe,
  evidenceDirectory,
  element,
  pairFreshProfiles,
  showMainWindow,
} from '../helpers/dualPeer.js'

async function saveSanitizedScreenshot(instance, destination) {
  await instance.tauri.switchWindow('main')
  await showMainWindow(instance)
  await instance.execute(() => {
    for (const item of document.querySelectorAll('body *')) {
      if (item.childElementCount > 0) continue
      if (
        /\b[\w.-]+\.local\b|\b[0-9a-f]{4,}…[0-9a-f]{4,}\b|\b[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\b/i.test(
          item.textContent
        )
      ) {
        item.style.filter = 'blur(12px)'
      }
    }
  })
  await instance.saveScreenshot(destination)
  expect(fs.statSync(destination).size).toBeGreaterThan(10_000)
}

async function unpair(connection, peerId) {
  const response = await daemonRequest(connection, '/pairing/unpair', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ peerId }),
  })
  expect(response.status).toBe(200)
  return (await response.json()).data
}

async function deviceTrust(connection) {
  const response = await daemonRequest(connection, '/member/device-group-choices')
  expect(response.status).toBe(200)
  return (await response.json()).data.deviceTrust
}

dualDescribe('removed device notification delivery', () => {
  it('keeps completed space status while showing a read-only removed device', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const evidenceDir = evidenceDirectory('removal-notification')

    await pairFreshProfiles({
      sponsor,
      joiner,
      passphrase: 'isolated-removal-notification-passphrase',
    })
    await click(joiner, '[data-testid="setup-complete-done"]')
    await click(sponsor, '[data-testid="setup-complete-done"]')
    await Promise.all([
      element(joiner, '[data-testid="history-preview-motion"]'),
      element(sponsor, '[data-testid="history-preview-motion"]'),
    ])
    await click(sponsor, 'a[href="/devices"]')
    const connection = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE)
    const joinerConnection = daemonConnection(process.env.E2E_UC_JOINER_PROFILE)
    const membersResponse = await daemonRequest(connection, '/paired-devices')
    expect(membersResponse.status).toBe(200)
    const members = (await membersResponse.json()).data
    expect(members).toHaveLength(1)
    const peerId = members[0].peerId
    await click(sponsor, `[data-testid="device-peer-${peerId}"]`)

    process.kill(joinerConnection.pid, 'SIGSTOP')
    try {
      await click(sponsor, '[data-testid="device-unpair"]')
      await click(sponsor, '[data-testid="device-unpair-confirm"]')

      let first
      await sponsor.waitUntil(
        async () => {
          first = await deviceTrust(connection)
          return first.devices.some(
            device =>
              device.deviceId === peerId &&
              device.membership === 'removed' &&
              device.groupRelationship === 'awaiting_removal_acknowledgement'
          )
        },
        { timeout: 30000, timeoutMsg: 'removed device state did not become publicly observable' }
      )
      const removed = first.devices.find(device => device.deviceId === peerId)
      expect(removed.membership).toBe('removed')
      expect(removed.groupRelationship).toBe('awaiting_removal_acknowledgement')
      expect(first.spaceDeviceUpdate.phase).toBe('completed')

      const repeated = await unpair(connection, peerId)
      expect(repeated.revision).toBe(first.revision)
      expect(repeated.devices.find(device => device.deviceId === peerId)).toEqual(removed)

      await sponsor.waitUntil(
        async () => (await sponsor.$$('[data-testid^="device-peer-"]')).length === 0,
        { timeout: 30000, timeoutMsg: 'removed device remained in the current member list' }
      )
      const removedRow = await element(sponsor, `[data-testid="removed-device-${peerId}"]`)
      expect(await removedRow.getAttribute('data-status')).toBe('removal_notification_pending')
      expect(await sponsor.$('[data-testid="space-device-update-status"]').isExisting()).toBe(false)
      await sponsor.waitUntil(
        async () => !(await sponsor.$('[data-testid="device-unpair-confirm"]').isDisplayed()),
        { timeout: 30000, timeoutMsg: 'remove confirmation dialog did not close after completion' }
      )

      await click(sponsor, `[data-testid="removed-device-${peerId}"]`)
      await expect(await element(sponsor, '[data-testid="removed-device-detail"]')).toExist()
      await expect(await element(sponsor, '[data-testid="removed-device-notice"]')).toExist()
      expect(await sponsor.$('[data-testid="device-unpair"]').isExisting()).toBe(false)
      await saveSanitizedScreenshot(sponsor, path.join(evidenceDir, 'removed-notifying.png'))
    } finally {
      process.kill(joinerConnection.pid, 'SIGCONT')
    }
  })
})
