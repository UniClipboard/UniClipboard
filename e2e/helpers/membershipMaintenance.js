import { randomBytes } from 'node:crypto'
import { mkdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { browser, expect } from '@wdio/globals'
import {
  click,
  daemonConnection,
  element,
  enterInvitation,
  initializeSponsor,
  issueInvitation,
  openFreshSetup,
  pairingComplete,
  showMainWindow,
} from './dualPeer.js'

async function session(connection) {
  const response = await fetch(`${connection.baseUrl}/auth/dev-token?pid=${process.pid}`, {
    method: 'POST',
  })
  if (!response.ok) throw new Error(`test session request returned ${response.status}`)
  return (await response.json()).sessionToken
}

async function request(connection, token, endpoint, method, body) {
  const response = await fetch(`${connection.baseUrl}${endpoint}`, {
    method,
    headers: {
      authorization: `Session ${token}`,
      'content-type': 'application/json',
      'x-uc-e2e-space-work-token': process.env.E2E_SPACE_WORK_TOKEN,
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
    signal: AbortSignal.timeout(120000),
  })
  if (!response.ok) throw new Error(`${method} ${endpoint} returned ${response.status}`)
  return response.status === 204 ? null : response.json()
}

async function screenshot(instance, destination) {
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

export async function runMaintenanceScenario(failure) {
  const sponsor = browser.sponsor
  const joiner = browser.joiner
  const passphrase = randomBytes(24).toString('hex')
  const evidenceDir = path.join(
    process.cwd(),
    '.herdr-project',
    'uni-t-0028',
    'evidence',
    `maintenance-${failure}-${process.pid}`
  )
  mkdirSync(evidenceDir, { recursive: true })
  await openFreshSetup(sponsor, joiner)
  await initializeSponsor(sponsor, passphrase)
  const code = await issueInvitation(sponsor)
  const connection = daemonConnection(process.env.E2E_UC_SPONSOR_PROFILE)
  const auth = await session(connection)
  const work = body => request(connection, auth, '/e2e/space-work', 'POST', body)
  const health = async () =>
    (await request(connection, auth, '/member/device-group-choices', 'GET')).data.deviceTrust
      .maintenanceHealth
  const count = failure === 'retryable' ? 2 : 1024
  const armed = await work({ command: 'arm_membership_history_failures', failure, count })
  expect(typeof armed.after_sequence).toBe('number')

  await click(joiner, '[data-testid="setup-entry-join"]')
  await enterInvitation(joiner, code, passphrase)
  await click(joiner, '[data-testid="setup-redeem-submit"]')
  const failureKind =
    failure === 'retryable'
      ? 'membership_history_sync_retryable_failure'
      : 'membership_history_sync_needs_attention'
  const failed = await work({
    command: 'wait_space_work_event',
    kind: failureKind,
    after_sequence: armed.after_sequence,
  })
  const expectedPhase = failure === 'retryable' ? 'retrying' : 'needs_attention'
  let affected
  await sponsor.waitUntil(
    async () => {
      affected = await health()
      return affected.phase === expectedPhase
    },
    { timeout: 30000, timeoutMsg: `公开维护状态未进入 ${expectedPhase}` }
  )
  if (failure === 'retryable') {
    expect(Number.isSafeInteger(affected.nextRetryAtMs)).toBe(true)
    expect(affected.reason).toBe(null)
    expect(affected.recovery).toBe(null)
  } else {
    expect(affected.reason).toBe('membership_history_rejected')
    expect(affected.recovery).toBe('resolve_device_trust')
    expect(affected.nextRetryAtMs).toBe(null)
  }

  await expect(await pairingComplete(sponsor, 'Sponsor')).toExist()
  await click(sponsor, '[data-testid="setup-complete-done"]')
  await element(sponsor, '[data-testid="history-preview-motion"]')
  await click(sponsor, 'a[href="/devices"]')
  const alert = await element(sponsor, '[data-testid="membership-maintenance-alert"]')
  await expect(alert).toExist()
  const text = await alert.getText()
  if (failure === 'retryable') {
    expect(text).toContain('自动重试')
    expect(await alert.$('button').isExisting()).toBe(false)
  } else {
    expect(text).toContain('请检查设备组')
    expect(await alert.$('button').isExisting()).toBe(true)
  }
  expect(text).not.toContain('Joiner')
  expect(text).not.toContain('Sponsor')
  expect(text).not.toContain('加入中')
  expect(text).not.toContain('更新空间')
  await screenshot(sponsor, path.join(evidenceDir, 'affected.png'))

  let remaining = null
  if (failure === 'needs_attention') {
    remaining = (await work({ command: 'clear_membership_history_failures' })).remaining
    expect(remaining).toBeGreaterThan(0)
    expect(remaining).toBeLessThan(count)
    await request(connection, auth, '/presence/opportunity', 'POST', {
      reason: 'network_changed',
    })
  }
  const reply = await work({
    command: 'wait_space_work_event',
    kind: 'membership_history_sync_reply_received',
    after_sequence: failed.sequence,
  })
  let recovered
  await sponsor.waitUntil(
    async () => {
      recovered = await health()
      return recovered.phase === 'healthy'
    },
    { timeout: 30000, timeoutMsg: '真实回复后公开维护状态未恢复正常' }
  )
  await alert.waitForExist({ reverse: true, timeout: 30000 })
  await screenshot(sponsor, path.join(evidenceDir, 'healthy.png'))
  const events = await work({ command: 'space_work_events' })
  const observed = events
    .filter(event => event.sequence > armed.after_sequence && event.sequence <= reply.sequence)
    .filter(event => event.kind.startsWith('membership_history_sync_'))
  const failures = observed.filter(event => event.kind === failureKind).length
  expect(failures).toBe(count - (remaining ?? 0))
  expect(observed.at(0).kind).toBe('membership_history_sync_started')
  expect(observed.at(-1).kind).toBe('membership_history_sync_reply_received')
  expect(observed.findIndex(event => event.kind === failureKind)).toBeLessThan(
    observed.findIndex(event => event.kind === 'membership_history_sync_reply_received')
  )
  if (failure === 'retryable') {
    expect(observed.map(event => event.kind)).toEqual([
      'membership_history_sync_started',
      'membership_history_sync_retryable_failure',
      'membership_history_sync_started',
      'membership_history_sync_retryable_failure',
      'membership_history_sync_started',
      'membership_history_sync_reply_received',
    ])
  }
  writeFileSync(
    path.join(evidenceDir, 'events.json'),
    JSON.stringify(
      {
        failure,
        afterSequence: armed.after_sequence,
        observed,
        retryAtMs: affected.nextRetryAtMs,
        remaining,
        finalPhase: recovered.phase,
      },
      null,
      2
    )
  )
  console.log(
    'membership maintenance:',
    failure,
    observed,
    'remaining:',
    remaining,
    'public:',
    recovered.phase
  )
}
