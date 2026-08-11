import { execFileSync } from 'node:child_process'
import { closeSync, ftruncateSync, openSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { browser, expect } from '@wdio/globals'

const dualDescribe =
  browser.isMultiremote && process.platform === 'darwin' ? describe : describe.skip

async function element(instance, selector, { timeout = 30000 } = {}) {
  const target = await instance.$(selector)
  await target.waitForExist({ timeout })
  return target
}

async function click(instance, selector) {
  await element(instance, selector)
  const clicked = await instance.execute(targetSelector => {
    const button = document.querySelector(targetSelector)
    if (!(button instanceof HTMLElement)) return false
    button.click()
    return true
  }, selector)
  expect(clicked).toBe(true)
}

async function showMainWindow(instance) {
  const visible = await instance.execute(async () => {
    const label = window.__TAURI_INTERNALS__.metadata.currentWindow.label
    await window.__TAURI_INTERNALS__.invoke('plugin:window|show', { label })
    return window.__TAURI_INTERNALS__.invoke('plugin:window|is_visible', { label })
  })
  expect(visible).toBe(true)
}

async function setupEntry(instance, selector, label) {
  try {
    return await element(instance, selector, { timeout: 60000 })
  } catch (error) {
    const state = await instance.execute(() => ({
      text: document.body?.innerText ?? '',
      testIds: Array.from(document.querySelectorAll('[data-testid]'), node =>
        node.getAttribute('data-testid')
      ),
      events: window.__WDIO_E2E_EVENTS__ ?? [],
    }))
    console.error(`${label} setup entry diagnostics:`, state)
    throw error
  }
}

async function invitationCode(instance) {
  let display
  try {
    display = await element(instance, '[data-testid="setup-invitation-code"]', {
      timeout: 60000,
    })
  } catch (error) {
    const state = await instance.execute(async () => {
      const setupRequests = performance
        .getEntriesByType('resource')
        .filter(entry => entry.name.includes('/v2/setup/'))
        .map(entry => {
          const url = new URL(entry.name)
          return {
            path: url.pathname,
            duration: entry.duration,
            responseEnd: entry.responseEnd,
          }
        })
      const issueRequest = performance
        .getEntriesByType('resource')
        .find(entry => entry.name.includes('/v2/setup/issue-invitation'))
      let stateProbe = null
      if (issueRequest) {
        const stateUrl = new URL(issueRequest.name)
        stateUrl.pathname = '/v2/setup/state'
        const response = await fetch(stateUrl, { signal: AbortSignal.timeout(5000) })
        const body = await response.json()
        stateProbe = {
          status: response.status,
          hasCompleted: body?.data?.hasCompleted,
          hasInvitation: body?.data?.currentInvitation != null,
        }
      }
      return {
        text: document.body?.innerText ?? '',
        testIds: Array.from(document.querySelectorAll('[data-testid]'), node =>
          node.getAttribute('data-testid')
        ),
        setupRequests,
        stateProbe,
        events: window.__WDIO_E2E_EVENTS__ ?? [],
      }
    })
    console.error('Sponsor invitation screen diagnostics:', state)
    throw error
  }
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

async function pairingComplete(instance, label) {
  try {
    return await element(instance, '[data-testid="setup-pairing-complete"]', {
      timeout: 90000,
    })
  } catch (error) {
    const state = await instance.execute(() => ({
      text: document.body?.innerText ?? '',
      testIds: Array.from(document.querySelectorAll('[data-testid]'), node =>
        node.getAttribute('data-testid')
      ),
      events: window.__WDIO_E2E_EVENTS__ ?? [],
    }))
    console.error(`${label} pairing completion diagnostics:`, state)
    throw error
  }
}

function daemonConnection(profile) {
  const dataDir = path.join(
    process.env.HOME,
    'Library',
    'Application Support',
    `app.uniclipboard.desktop-${profile}`
  )
  // ADR-011: the daemon binds an ephemeral port and publishes port + token in
  // `daemon.conn`; retry until the file appears.
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const conn = JSON.parse(readFileSync(path.join(dataDir, 'daemon.conn'), 'utf8'))
      if (conn.port && conn.token) {
        return {
          baseUrl: `http://${conn.host ?? '127.0.0.1'}:${conn.port}`,
          token: conn.token.trim(),
          sessionToken: null,
        }
      }
    } catch {
      // not published yet — keep polling
    }
    sleepSync(200)
  }
  throw new Error(`daemon.conn not published for profile ${profile} at ${dataDir}`)
}

function sleepSync(ms) {
  const buffer = new SharedArrayBuffer(4)
  Atomics.wait(new Int32Array(buffer), 0, 0, ms)
}

async function daemonRequest(connection, requestPath, options = {}) {
  if (connection.sessionToken === null) {
    const connectResponse = await fetch(`${connection.baseUrl}/auth/connect`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${connection.token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ pid: process.pid, clientType: 'e2e-cancel-repro' }),
    })
    expect(connectResponse.status).toBe(200)
    connection.sessionToken = (await connectResponse.json()).data.sessionToken
  }
  const response = await fetch(`${connection.baseUrl}${requestPath}`, {
    ...options,
    headers: {
      Authorization: `Session ${connection.sessionToken}`,
      ...(options.headers ?? {}),
    },
  })
  return response
}

async function encryptionState(connection) {
  const response = await daemonRequest(connection, '/encryption/state')
  expect(response.status).toBe(200)
  return (await response.json()).data
}

async function unlockPeer(instance, connection, passphrase) {
  const state = await encryptionState(connection)
  if (state.sessionReady) return

  await click(instance, 'button=Unlock')
  const passphraseInput = await element(instance, '#unlock-passphrase')
  await passphraseInput.setValue(passphrase)
  await instance.keys('Enter')

  await instance.waitUntil(async () => (await encryptionState(connection)).sessionReady === true, {
    timeout: 60000,
    timeoutMsg: 'encryption session did not become ready after GUI unlock',
  })
}

async function waitForPairedPeer(instance, connection) {
  await instance.waitUntil(
    async () => {
      const response = await daemonRequest(connection, '/paired-devices')
      if (response.status !== 200) return false
      return (await response.json()).data.length === 1
    },
    { timeout: 60000, timeoutMsg: 'the paired peer never became available' }
  )
}

function copyFileToSystemClipboard(filePath) {
  const script = [
    'ObjC.import("AppKit")',
    'const pasteboard = $.NSPasteboard.generalPasteboard',
    `const fileUrl = $.NSURL.fileURLWithPath($(${JSON.stringify(filePath)}))`,
    'pasteboard.clearContents',
    'if (!pasteboard.writeObjects($([fileUrl]))) throw new Error("could not write file URL to system clipboard")',
  ].join('; ')
  execFileSync('/usr/bin/osascript', ['-l', 'JavaScript', '-e', script])
}

function createTransferFile(filePath) {
  const descriptor = openSync(filePath, 'w')
  try {
    ftruncateSync(descriptor, 512 * 1024 * 1024)
  } finally {
    closeSync(descriptor)
  }
}

dualDescribe('同机双客户端首次配对', () => {
  it('拒绝失效邀请码和错误口令后仍可用全新邀请码完成加入', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-dual-peer-passphrase'

    await Promise.all([sponsor.tauri.switchWindow('main'), joiner.tauri.switchWindow('main')])
    await Promise.all([showMainWindow(sponsor), showMainWindow(joiner)])

    await Promise.all([
      setupEntry(sponsor, '[data-testid="setup-entry-create"]', 'Sponsor'),
      setupEntry(joiner, '[data-testid="setup-entry-join"]', 'Joiner'),
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

    await click(sponsor, '[data-testid="setup-complete-invite"]')
    const finalCode = await invitationCode(sponsor)
    expect(finalCode).not.toBe(activeCode)

    await enterInvitation(joiner, finalCode, passphrase)
    await click(joiner, '[data-testid="setup-redeem-submit"]')

    const [sponsorComplete, joinerComplete] = await Promise.all([
      pairingComplete(sponsor, 'Sponsor'),
      pairingComplete(joiner, 'Joiner'),
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

  it('allows the receiving history entry to be cancelled while its file download is active', async () => {
    const sponsor = browser.sponsor
    const joiner = browser.joiner
    const passphrase = 'e2e-dual-peer-passphrase'
    const sponsorConnection = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE ?? 'wdio-sponsor')
    const joinerConnection = daemonConnection(process.env.E2E_UC_JOINER_PROFILE ?? 'wdio-joiner')
    let phase = 'leave the setup completion screen'

    try {
      await Promise.all([
        click(sponsor, '[data-testid="setup-complete-done"]'),
        click(joiner, '[data-testid="setup-complete-done"]'),
      ])

      phase = 'wait for both history views to begin receiving live updates'
      await Promise.all([
        element(sponsor, '[data-testid="history-preview-motion"]'),
        element(joiner, '[data-testid="history-preview-motion"]'),
      ])
      await browser.pause(1000)

      phase = 'unlock both peers'
      await Promise.all([
        unlockPeer(sponsor, sponsorConnection, passphrase),
        unlockPeer(joiner, joinerConnection, passphrase),
      ])
      phase = 'wait for the unlocked history views to subscribe to live updates'
      await browser.pause(1000)
      phase = 'confirm both peers are paired'
      await Promise.all([
        waitForPairedPeer(sponsor, sponsorConnection),
        waitForPairedPeer(joiner, joinerConnection),
      ])

      phase = 'create and copy the transfer file'
      const transferFile = path.join(tmpdir(), 'uniclip-e2e-cancel-repro.bin')
      createTransferFile(transferFile)
      copyFileToSystemClipboard(transferFile)

      phase = 'capture the copied file'
      const captureResponse = await daemonRequest(sponsorConnection, '/clipboard/capture-current', {
        method: 'POST',
      })
      expect(captureResponse.status).toBe(200)
      const capture = await captureResponse.json()
      expect(capture.data.entryId).toBeTruthy()

      phase = 'wait for the copied file to arrive at the receiving peer'
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

      phase = 'confirm the active transfer in history and click cancel'
      const detail = await element(joiner, '[data-testid="clipboard-detail"]')
      await expect(detail).toHaveText(expect.stringContaining('uniclip-e2e-cancel-repro.bin'))
      const cancelButton = await detail.$('[aria-label="取消传输"]')
      await cancelButton.waitForExist({ timeout: 30000 })
      await cancelButton.click()

      phase = 'wait for cancellation to finish'
      await joiner.waitUntil(
        async () => {
          const response = await daemonRequest(joinerConnection, '/clipboard/receives')
          if (response.status !== 200) return false
          return !(await response.json()).data.some(item => item.entryId === receive.entryId)
        },
        { timeout: 60000, timeoutMsg: 'receive remained active after the GUI cancel action' }
      )
      phase = 'remove the cancelled temporary history entry'
      await expect(await joiner.$('[data-testid="clipboard-detail"]')).not.toExist()
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      throw new Error(`file cancellation test failed while trying to ${phase}: ${message}`, {
        cause: error,
      })
    }
  })
})
