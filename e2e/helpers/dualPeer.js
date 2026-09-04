import { execFileSync } from 'node:child_process'
import { closeSync, ftruncateSync, openSync, readFileSync } from 'node:fs'
import { browser, expect } from '@wdio/globals'

export const dualDescribe =
  browser.isMultiremote && process.platform === 'darwin' ? describe : describe.skip

export async function element(instance, selector, { timeout = 30000 } = {}) {
  const target = await instance.$(selector)
  await target.waitForExist({ timeout })
  return target
}

export async function click(instance, selector) {
  await element(instance, selector)
  const clicked = await instance.execute(targetSelector => {
    const button = document.querySelector(targetSelector)
    if (!(button instanceof HTMLElement)) return false
    button.click()
    return true
  }, selector)
  expect(clicked).toBe(true)
}

export async function showMainWindow(instance) {
  const visible = await instance.execute(async () => {
    const label = window.__TAURI_INTERNALS__.metadata.currentWindow.label
    await window.__TAURI_INTERNALS__.invoke('plugin:window|show', { label })
    return window.__TAURI_INTERNALS__.invoke('plugin:window|is_visible', { label })
  })
  expect(visible).toBe(true)
}

export async function setupEntry(instance, selector, label) {
  try {
    return await element(instance, selector, { timeout: 60000 })
  } catch (error) {
    const state = await pageDiagnostics(instance)
    console.error(`${label} setup entry diagnostics:`, state)
    throw error
  }
}

export async function openFreshSetup(sponsor, joiner) {
  await Promise.all([sponsor.tauri.switchWindow('main'), joiner.tauri.switchWindow('main')])
  await Promise.all([showMainWindow(sponsor), showMainWindow(joiner)])
  await Promise.all([
    setupEntry(sponsor, '[data-testid="setup-entry-create"]', 'Sponsor'),
    setupEntry(joiner, '[data-testid="setup-entry-join"]', 'Joiner'),
  ])
}

export async function initializeSponsor(sponsor, passphrase, deviceName = 'E2E Sponsor') {
  await click(sponsor, '[data-testid="setup-entry-create"]')
  await (await element(sponsor, '#device-name')).setValue(deviceName)
  await (await element(sponsor, '#pass1')).setValue(passphrase)
  await (await element(sponsor, '#pass2')).setValue(passphrase)
  await click(sponsor, '[data-testid="setup-initialize-submit"]')
}

export async function invitationCode(instance) {
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

export async function issueInvitation(sponsor) {
  await click(sponsor, '[data-testid="setup-complete-invite"]')
  return invitationCode(sponsor)
}

export async function enterInvitation(instance, code, passphrase) {
  const codeInput = await element(instance, '#join-code')
  await codeInput.setValue(code)
  expect(await codeInput.getValue()).toBe(code)
  const passphraseInput = await element(instance, '#join-pass')
  expect(await passphraseInput.getValue()).toBe('')
  await passphraseInput.setValue(passphrase)
}

export async function pairingComplete(instance, label) {
  try {
    return await element(instance, '[data-testid="setup-pairing-complete"]', {
      timeout: 30000,
    })
  } catch (error) {
    console.error(`${label} pairing completion diagnostics:`, await pageDiagnostics(instance))
    throw error
  }
}

export async function pairFreshProfiles({ sponsor, joiner, passphrase }) {
  await openFreshSetup(sponsor, joiner)
  await initializeSponsor(sponsor, passphrase)
  const code = await issueInvitation(sponsor)
  await click(joiner, '[data-testid="setup-entry-join"]')
  await enterInvitation(joiner, code, passphrase)
  const startedAt = Date.now()
  await click(joiner, '[data-testid="setup-redeem-submit"]')
  const [sponsorComplete, joinerComplete] = await Promise.all([
    pairingComplete(sponsor, 'Sponsor'),
    pairingComplete(joiner, 'Joiner'),
  ])
  await expect(sponsorComplete).toExist()
  await expect(joinerComplete).toExist()
  return { elapsedMs: Date.now() - startedAt }
}

export function daemonConnection(profile) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const connection = readDaemonConnection(profile)
    if (connection) return connection
    sleepSync(200)
  }
  throw new Error(`daemon.conn not published for profile ${profile}`)
}

export function readDaemonConnection(profile) {
  const dataDir = `${process.env.HOME}/Library/Application Support/app.uniclipboard.desktop-${profile}`
  try {
    const conn = JSON.parse(readFileSync(`${dataDir}/daemon.conn`, 'utf8'))
    if (!conn.port || !conn.token || !conn.pid) return null
    return {
      baseUrl: `http://${conn.host ?? '127.0.0.1'}:${conn.port}`,
      token: conn.token.trim(),
      pid: conn.pid,
      sessionToken: null,
    }
  } catch {
    return null
  }
}

export async function waitForDaemonReplacement(instance, profile, previousPid) {
  let replacement = null
  await instance.waitUntil(
    async () => {
      replacement = readDaemonConnection(profile)
      return replacement !== null && replacement.pid !== previousPid
    },
    { timeout: 30000, timeoutMsg: `daemon did not restart for profile ${profile}` }
  )
  return replacement
}

export async function waitForDaemonUnreachable(instance, connection) {
  await instance.waitUntil(
    async () => {
      try {
        await fetch(`${connection.baseUrl}/health`, { signal: AbortSignal.timeout(500) })
        return false
      } catch {
        return true
      }
    },
    { timeout: 10000, timeoutMsg: `daemon at ${connection.baseUrl} remained reachable` }
  )
}

function sleepSync(ms) {
  const buffer = new SharedArrayBuffer(4)
  Atomics.wait(new Int32Array(buffer), 0, 0, ms)
}

export async function daemonRequest(connection, requestPath, options = {}) {
  if (connection.sessionToken === null) {
    const connectResponse = await fetch(`${connection.baseUrl}/auth/connect`, {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${connection.token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ pid: process.pid, clientType: 'gui-e2e' }),
    })
    expect(connectResponse.status).toBe(200)
    connection.sessionToken = (await connectResponse.json()).data.sessionToken
  }
  return fetch(`${connection.baseUrl}${requestPath}`, {
    ...options,
    headers: {
      Authorization: `Session ${connection.sessionToken}`,
      ...(options.headers ?? {}),
    },
  })
}

async function encryptionState(connection) {
  const response = await daemonRequest(connection, '/encryption/state')
  expect(response.status).toBe(200)
  return (await response.json()).data
}

export async function unlockPeer(instance, connection, passphrase) {
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

export async function waitForPairedPeer(instance, connection) {
  await instance.waitUntil(
    async () => {
      const response = await daemonRequest(connection, '/paired-devices')
      if (response.status !== 200) return false
      return (await response.json()).data.length === 1
    },
    { timeout: 60000, timeoutMsg: 'the paired peer never became available' }
  )
}

export function copyFileToSystemClipboard(filePath) {
  const script = [
    'ObjC.import("AppKit")',
    'const pasteboard = $.NSPasteboard.generalPasteboard',
    `const fileUrl = $.NSURL.fileURLWithPath($(${JSON.stringify(filePath)}))`,
    'pasteboard.clearContents',
    'if (!pasteboard.writeObjects($([fileUrl]))) throw new Error("could not write file URL to system clipboard")',
  ].join('; ')
  execFileSync('/usr/bin/osascript', ['-l', 'JavaScript', '-e', script])
}

export function createTransferFile(filePath) {
  const descriptor = openSync(filePath, 'w')
  try {
    ftruncateSync(descriptor, 512 * 1024 * 1024)
  } finally {
    closeSync(descriptor)
  }
}

export async function pageDiagnostics(instance) {
  return instance.execute(() => ({
    text: document.body?.innerText ?? '',
    testIds: Array.from(document.querySelectorAll('[data-testid]'), node =>
      node.getAttribute('data-testid')
    ),
    events: window.__WDIO_E2E_EVENTS__ ?? [],
  }))
}
