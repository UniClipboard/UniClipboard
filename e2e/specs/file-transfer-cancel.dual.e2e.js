import { tmpdir } from 'node:os'
import path from 'node:path'
import { browser, expect } from '@wdio/globals'
import {
  click,
  copyFileToSystemClipboard,
  createTransferFile,
  daemonConnection,
  daemonRequest,
  dualDescribe,
  element,
  pairFreshProfiles,
  pageDiagnostics,
  unlockPeer,
  waitForPairedPeer,
} from '../helpers/dualPeer.js'

dualDescribe('接收中的文件传输', () => {
  it('可以从接收端历史详情取消活动下载', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-file-cancel-passphrase'
    await pairFreshProfiles({ sponsor, joiner, passphrase })

    const sponsorConnection = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE)
    const joinerConnection = daemonConnection(process.env.E2E_UC_JOINER_PROFILE)
    await Promise.all([
      click(sponsor, '[data-testid="setup-complete-done"]'),
      click(joiner, '[data-testid="setup-complete-done"]'),
    ])
    try {
      await Promise.all([
        element(sponsor, '[data-testid="history-preview-motion"]'),
        element(joiner, '[data-testid="history-preview-motion"]'),
      ])
    } catch (error) {
      console.error('Sponsor history diagnostics:', await pageDiagnostics(sponsor))
      console.error('Joiner history diagnostics:', await pageDiagnostics(joiner))
      throw error
    }
    await Promise.all([
      unlockPeer(sponsor, sponsorConnection, passphrase),
      unlockPeer(joiner, joinerConnection, passphrase),
    ])
    await Promise.all([
      waitForPairedPeer(sponsor, sponsorConnection),
      waitForPairedPeer(joiner, joinerConnection),
    ])

    const transferFile = path.join(tmpdir(), `uniclip-e2e-cancel-${Date.now()}.bin`)
    createTransferFile(transferFile)
    copyFileToSystemClipboard(transferFile)
    const captureResponse = await daemonRequest(sponsorConnection, '/clipboard/capture-current', {
      method: 'POST',
    })
    expect(captureResponse.status).toBe(200)

    let receive = null
    await joiner.waitUntil(
      async () => {
        const response = await daemonRequest(joinerConnection, '/clipboard/receives')
        if (response.status !== 200) return false
        receive = (await response.json()).data.find(item => item.state === 'receiving') ?? null
        return receive !== null
      },
      { timeout: 60000, timeoutMsg: 'joiner never exposed an active file receive' }
    )

    const detail = await element(joiner, '[data-testid="clipboard-detail"]')
    await expect(detail).toHaveText(expect.stringContaining(path.basename(transferFile)))
    const cancelButton = await detail.$('[aria-label="取消传输"]')
    await cancelButton.waitForExist({ timeout: 30000 })
    await cancelButton.click()
    await joiner.waitUntil(
      async () => {
        const response = await daemonRequest(joinerConnection, '/clipboard/receives')
        if (response.status !== 200) return false
        return !(await response.json()).data.some(item => item.entryId === receive.entryId)
      },
      { timeout: 60000, timeoutMsg: 'receive remained active after the GUI cancel action' }
    )
    await expect(await joiner.$('[data-testid="clipboard-detail"]')).not.toExist()
  })
})
