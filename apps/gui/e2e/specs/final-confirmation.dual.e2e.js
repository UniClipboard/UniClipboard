import path from 'node:path'
import { browser, expect } from '@wdio/globals'
import {
  click,
  daemonConnection,
  dualDescribe,
  evidenceDirectory,
  element,
  enterInvitation,
  initializeSponsor,
  issueInvitation,
  openFreshSetup,
  pairingComplete,
  showMainWindow,
} from '../helpers/dualPeer.js'

async function spaceWork(connection, command, afterSequence, kind) {
  const tokenResponse = await fetch(`${connection.baseUrl}/auth/dev-token?pid=${process.pid}`, {
    method: 'POST',
  })
  expect(tokenResponse.ok).toBe(true)
  const { sessionToken } = await tokenResponse.json()
  const response = await fetch(`${connection.baseUrl}/e2e/space-work`, {
    method: 'POST',
    headers: {
      authorization: `Session ${sessionToken}`,
      'content-type': 'application/json',
      'x-uc-e2e-space-work-token': process.env.E2E_SPACE_WORK_TOKEN,
    },
    body: JSON.stringify({ command, after_sequence: afterSequence, kind }),
    signal: AbortSignal.timeout(120000),
  })
  expect(response.status).toBe(200)
  return response.json()
}

async function saveSanitizedScreenshot(instance, destination) {
  await instance.tauri.switchWindow('main')
  await showMainWindow(instance)
  await instance.execute(() => {
    for (const item of document.querySelectorAll('body *')) {
      if (item.childElementCount > 0) continue
      if (/\b[\w.-]+\.local\b|\b[0-9a-f]{4,}…[0-9a-f]{4,}\b/i.test(item.textContent)) {
        item.style.filter = 'blur(12px)'
      }
    }
  })
  await instance.saveScreenshot(destination)
}

dualDescribe('最终确认失败后的加入界面', () => {
  it('失败时仍显示处理中，到期直接重试并完成加入', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'isolated-final-confirmation-passphrase'
    const evidenceDir = evidenceDirectory('final-confirmation-gui')

    await openFreshSetup(sponsor, joiner)
    await initializeSponsor(sponsor, passphrase)
    const code = await issueInvitation(sponsor)
    const control = daemonConnection(process.env.E2E_UC_JOINER_PROFILE)
    const arm = await spaceWork(control, 'arm_complete_ack_failure')
    expect(typeof arm.after_sequence).toBe('number')

    await click(joiner, '[data-testid="setup-entry-join"]')
    await enterInvitation(joiner, code, passphrase)
    await click(joiner, '[data-testid="setup-redeem-submit"]')
    const failed = await spaceWork(
      control,
      'wait_space_work_event',
      arm.after_sequence,
      'final_confirmation_connection_failed'
    )
    await expect(
      await element(joiner, '[data-testid="setup-join-processing"]', { timeout: 20000 })
    ).toExist()
    expect(await joiner.$('[data-testid="setup-pairing-complete"]').isExisting()).toBe(false)
    await joiner.saveScreenshot(path.join(evidenceDir, 'failed-waiting.png'))

    const retry = await spaceWork(
      control,
      'wait_space_work_event',
      failed.sequence,
      'final_confirmation_retry_started'
    )
    const events = await spaceWork(control, 'space_work_events')
    const between = events.filter(
      event => event.sequence > failed.sequence && event.sequence < retry.sequence
    )
    const counts = {
      ordinary: between.filter(event => event.kind === 'ordinary_member_update_started').length,
      history: between.filter(event => event.kind === 'membership_history_sync_started').length,
    }
    console.log(
      'final confirmation event sequence:',
      failed.sequence,
      between,
      retry.sequence,
      counts
    )
    expect(counts).toEqual({ ordinary: 0, history: 0 })
    await spaceWork(
      control,
      'wait_space_work_event',
      retry.sequence,
      'final_confirmation_reply_received'
    )

    await expect(await pairingComplete(joiner, 'Joiner')).toExist()
    await expect(await pairingComplete(sponsor, 'Sponsor')).toExist()
    await click(joiner, '[data-testid="setup-complete-done"]')
    await click(sponsor, '[data-testid="setup-complete-done"]')
    await Promise.all([
      element(joiner, '[data-testid="history-preview-motion"]'),
      element(sponsor, '[data-testid="history-preview-motion"]'),
    ])
    await Promise.all([click(joiner, 'a[href="/devices"]'), click(sponsor, 'a[href="/devices"]')])
    await expect(
      await element(joiner, '[data-testid="device-local"]', { timeout: 30000 })
    ).toExist()
    await expect(
      await element(sponsor, '[data-testid^="device-peer-"]', { timeout: 30000 })
    ).toExist()
    const peerCount = await sponsor.$$('[data-testid^="device-peer-"]')
    expect(peerCount).toHaveLength(1)
    await sponsor.waitUntil(
      async () => {
        const status = await sponsor.$('[data-testid^="device-peer-"]').getAttribute('data-status')
        return status === 'online' || status === 'offline'
      },
      {
        timeout: 30000,
        timeoutMsg: '设备列表在加入完成后仍显示等待对方确认',
      }
    )
    await saveSanitizedScreenshot(sponsor, path.join(evidenceDir, 'members.png'))
  })
})
