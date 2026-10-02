import assert from 'node:assert/strict'
import { mkdir, appendFile } from 'node:fs/promises'
import { join } from 'node:path'
import { request, waitFor, connect } from './conflict-userdata.mjs'

const issueLabels = new Map()
export function describeGroups(groups, run) {
  const role = id => {
    const index = run.expected.findIndex(state => state?.deviceTrust.localDeviceId === id)
    return index < 0 ? 'new-member' : 'abcde'[index]
  }
  return {
    revision: groups.revision,
    devices: groups.deviceTrust.devices.map(device => ({
      role: role(device.deviceId),
      membership: device.membership,
      relationship: device.groupRelationship,
      sync: device.syncRelationship,
    })),
    issues: groups.issues.map(issue => {
      if (!issueLabels.has(issue.issueId))
        issueLabels.set(issue.issueId, `issue-${issueLabels.size + 1}`)
      return {
        label: issueLabels.get(issue.issueId),
        kind: issue.reason?.kind ?? 'unknown',
        changes: issue.reason?.changes.map(change => ({
          actor: role(change.actor.deviceId),
          target: role(change.target.deviceId),
          kind: change.kind,
          side: change.side,
        })),
        options: issue.choices.map(choice => ({
          current: choice.isCurrentGroup,
          members: choice.memberDeviceIds.map(role),
          complete: choice.membersComplete,
        })),
      }
    }),
  }
}

export async function screenshot(instance, run, name) {
  await mkdir(join(run.runDir, 'screenshots'), { recursive: true })
  // Background WKWebViews can pause CSS animations; capture their final visual state.
  await instance.execute(() => {
    for (const animation of document.getAnimations())
      if (animation.effect?.getComputedTiming().iterations !== Infinity) animation.finish()
  })
  await instance.saveScreenshot(join(run.runDir, 'screenshots', `${name}.png`))
}
export async function visible(instance, selector) {
  return (await instance.$(selector)).isDisplayed()
}
export async function click(instance, selector) {
  const element = await instance.$(selector)
  await element.waitForExist({ timeout: 60000 })
  await element.waitForDisplayed({ timeout: 60000 })
  await element.waitForEnabled({ timeout: 60000 })
  await element.click()
}
export async function confirmSelection(instance, choiceId) {
  const read = () =>
    instance.execute(id => {
      if (
        document.querySelector(
          '[data-testid="device-trust-recheck"], [data-testid="device-trust-done"], [data-testid="device-trust-local-removal-warning"]'
        )
      )
        return 'submitted'
      const choice = document.querySelector(`[data-testid="device-trust-choice-${id}"]`)
      if (choice?.getAttribute('aria-checked') !== 'true') return 'review'
      const button = document.querySelector('[data-testid="device-trust-confirm"]')
      return button && !button.disabled ? 'ready' : 'waiting'
    }, choiceId)
  let state
  await instance.waitUntil(
    async () => {
      state = await read()
      return state !== 'waiting'
    },
    { timeout: 60000 }
  )
  if (state !== 'ready') return state === 'submitted'
  const button = await instance.$('[data-testid="device-trust-confirm"]')
  state = await read()
  if (state !== 'ready') return state === 'submitted'
  await button.click()
  await instance.waitUntil(
    async () => {
      state = await read()
      return state === 'submitted' || state === 'review'
    },
    { timeout: 60000 }
  )
  return state === 'submitted'
}
export async function recheckDecision(instance) {
  const done = '[data-testid="device-trust-done"]'
  const recheck = '[data-testid="device-trust-recheck"]'
  await instance.waitUntil(
    async () => {
      if (await visible(instance, done)) return true
      const button = await instance.$(recheck)
      if (!(await button.isExisting()) || !(await button.isEnabled())) return false
      try {
        await button.click()
      } catch (error) {
        // A notification can complete the decision between locating and clicking.
        if (!(await visible(instance, done))) throw error
      }
      return true
    },
    { timeout: 20000 }
  )
  await instance.waitUntil(
    async () => (await visible(instance, done)) || (await (await instance.$(recheck)).isEnabled()),
    { timeout: 20000 }
  )
}
export async function choose(instance, conn, run, selector, name, local = false) {
  await (
    await instance.$('[data-testid="device-trust-confirm"]')
  ).waitForDisplayed({ timeout: 90000 })
  await screenshot(instance, run, `${name}-before`)
  for (let attempt = 0; attempt < 6; attempt++) {
    const state = await request(conn, '/member/device-group-choices')
    const choice = state.issues[0]?.choices.find(selector)
    assert.ok(choice, 'requested candidate must exist')
    await appendFile(
      join(run.runDir, 'decisions.jsonl'),
      JSON.stringify({
        phase: 'before',
        role: conn.profile.slice(-1),
        selectedCurrent: choice.isCurrentGroup,
        ...describeGroups(state, run),
      }) + '\n',
      { mode: 0o600 }
    )
    await click(instance, `[data-testid="device-trust-choice-${choice.choiceId}"]`)
    await screenshot(instance, run, `${name}-selected`)
    if (!(await confirmSelection(instance, choice.choiceId))) {
      await screenshot(instance, run, `${name}-review-required-${attempt}`)
      continue
    }
    if (local) {
      await (
        await instance.$('[data-testid="device-trust-local-removal-warning"]')
      ).waitForDisplayed()
      await screenshot(instance, run, `${name}-local-confirm`)
      await click(instance, '[data-testid="device-trust-confirm"]')
    }
    await instance.waitUntil(
      async () =>
        instance.execute(
          () =>
            !!document.querySelector(
              '[data-testid="device-trust-done"], [data-error="device_state_changed"], [data-testid="device-trust-recheck"]:not(:disabled)'
            )
        ),
      { timeout: 90000 }
    )
    if (await visible(instance, '[data-error="device_state_changed"]')) {
      await screenshot(instance, run, `${name}-review-${attempt}`)
      continue
    }
    await screenshot(instance, run, `${name}-result`)
    // Recheck only the result, never resend a decision without an explicit review.
    for (
      let read = 0;
      read < 6 && !(await visible(instance, '[data-testid="device-trust-done"]'));
      read++
    ) {
      await recheckDecision(instance)
    }
    if (
      !(await visible(instance, '[data-testid="device-trust-done"]')) &&
      (await visible(instance, '[data-testid="device-trust-back"]'))
    ) {
      const latest = await request(conn, '/member/device-group-choices')
      assert.ok(
        latest.issues.some(issue => issue.issueId === state.issues[0].issueId),
        'review requires a current issue'
      )
      await screenshot(instance, run, `${name}-failed-review-${attempt}`)
      await click(instance, '[data-testid="device-trust-back"]')
      continue
    }
    await (
      await instance.$('[data-testid="device-trust-done"]')
    ).waitForDisplayed({ timeout: 90000 })
    const after = await request(conn, '/member/device-group-choices')
    await appendFile(
      join(run.runDir, 'decisions.jsonl'),
      JSON.stringify({
        phase: 'after',
        role: conn.profile.slice(-1),
        ...describeGroups(after, run),
      }) + '\n',
      { mode: 0o600 }
    )
    assert.ok(
      !after.issues.some(issue => issue.issueId === state.issues[0].issueId),
      'selected issue must resolve'
    )
    await screenshot(instance, run, `${name}-completed`)
    await click(instance, '[data-testid="device-trust-done"]')
    await instance.waitUntil(
      async () => !(await visible(instance, '[data-testid="device-trust-done"]')),
      { timeout: 20000 }
    )
    return { before: state, choice, after }
  }
  throw new Error('Choice kept changing after six explicit reviews')
}
export async function restartDaemon(instance, old) {
  await instance.execute(() => {
    void window.__TAURI_INTERNALS__.invoke('restart_daemon', { trace: null })
  })
  let next
  await waitFor(
    async () => {
      try {
        next = await connect(old.profile)
        return next.pid !== old.pid
      } catch {
        return false
      }
    },
    'daemon replacement',
    60000
  )
  await instance.execute(() => window.dispatchEvent(new Event('focus')))
  return next
}
