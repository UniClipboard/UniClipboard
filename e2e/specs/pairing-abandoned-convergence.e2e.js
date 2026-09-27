import fs from 'node:fs'
import path from 'node:path'
import { click, showMainWindow } from '../helpers/dualPeer.js'

const phase = process.env.ABANDONED_PAIRING_GUI_PHASE
const evidenceDir = process.env.ABANDONED_PAIRING_GUI_EVIDENCE_DIR
const passphrase = process.env.ABANDONED_PAIRING_PASSPHRASE

async function unlockIfNeeded() {
  const unlockButton = await $('[data-testid="unlock-content"]')
  if (!(await unlockButton.isExisting())) return

  await click(browser, '[data-testid="unlock-content"]')
  await browser.waitUntil(
    async () => {
      const deviceLink = await $('a[href="/devices"]')
      const passphraseInput = await $('#unlock-passphrase')
      return (await deviceLink.isExisting()) || (await passphraseInput.isDisplayed())
    },
    { timeout: 30000, timeoutMsg: 'unlock did not open the app or request a passphrase' }
  )

  const passphraseInput = await $('#unlock-passphrase')
  if (await passphraseInput.isDisplayed()) {
    await passphraseInput.setValue(passphrase)
    await browser.keys('Enter')
  }
}

async function openDevices() {
  await browser.tauri.switchWindow('main')
  await showMainWindow(browser)
  await unlockIfNeeded()
  try {
    await browser.waitUntil(
      async () => {
        if ((await browser.getUrl()).endsWith('/devices')) return true
        await browser.execute(() => {
          const link = document.querySelector('a[href="/devices"]')
          if (!(link instanceof HTMLElement)) return false
          link.click()
          return true
        })
        return (await browser.getUrl()).endsWith('/devices')
      },
      {
        timeout: 60000,
        interval: 250,
        timeoutMsg: 'device navigation did not become available',
      }
    )
  } catch (error) {
    await saveEvidence(`${phase}-navigation-timeout.png`)
    const url = await browser.getUrl()
    const body = (await $('body').getText()).replaceAll(/\s+/g, ' ').slice(0, 500)
    throw new Error(`device navigation failed at ${url}: ${body}`, { cause: error })
  }
  await browser.waitUntil(async () => (await browser.getUrl()).endsWith('/devices'), {
    timeout: 1000,
    timeoutMsg: 'device page did not open',
  })
  await $('[data-device-list]').waitForExist({ timeout: 30000 })
}

async function saveEvidence(name) {
  fs.mkdirSync(evidenceDir, { recursive: true })
  await browser.execute(() => {
    for (const item of document.querySelectorAll('body *')) {
      if (item.childElementCount > 0) continue
      if (/\b[\w.-]+\.local\b|\b[0-9a-f]{4,}…[0-9a-f]{4,}\b/i.test(item.textContent)) {
        item.style.filter = 'blur(12px)'
      }
    }
  })
  const target = path.join(evidenceDir, name)
  await browser.saveScreenshot(target)
  expect(fs.statSync(target).size).toBeGreaterThan(10_000)
}

describe('abandoned pairing convergence', () => {
  before(function () {
    if (!['red', 'green'].includes(phase) || !evidenceDir) this.skip()
    expect(passphrase).toBeTruthy()
  })

  it('keeps formal devices separate from unfinished pairing attempts', async () => {
    await openDevices()
    const peers = await $$('[data-testid^="device-peer-"]')

    if (phase === 'red') {
      expect(peers).toHaveLength(1)
      await $('[data-testid="device-trust-load-error"]').waitForDisplayed({ timeout: 30000 })
      expect(await $$('[data-testid^="inbound-pairing-"]')).toHaveLength(0)
      await saveEvidence('red-device-status-unavailable.png')
      return
    }

    expect(peers.length).toBeGreaterThanOrEqual(1)
    await browser.waitUntil(
      async () => {
        const failed = await $$('[data-testid="inbound-pairing-failed"]')
        const missed = await $$('[data-testid="inbound-pairing-confirmation_missed"]')
        return failed.length + missed.length === 2
      },
      { timeout: 30000, timeoutMsg: 'converged pairing attempts were not shown separately' }
    )
    expect(await $('[data-testid="device-trust-load-error"]').isExisting()).toBe(false)
    await saveEvidence('green-formal-device-and-candidates.png')
    for (const peer of peers) {
      expect(['online', 'offline']).toContain(await peer.getAttribute('data-status'))
    }
  })
})
