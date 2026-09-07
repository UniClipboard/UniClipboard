import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'
import { screenshot, click, visible } from './conflict-gui.js'
import { assertNativeKeyboardAvailable } from './conflict-native.mjs'
import { request } from './conflict-userdata.mjs'

function fixture(current, suffix, options = {}) {
  const groups = structuredClone(current)
  const local = groups.deviceTrust.localDeviceId
  groups.revision += 100
  const members = [
    { deviceId: local, displayName: '本机', isLocal: true, active: true },
    {
      deviceId: 'fixture-remote-0001',
      displayName: options.long ? '远端超长设备名称'.repeat(14) : 'Remote only',
      isLocal: false,
      active: true,
    },
  ]
  groups.issues = [
    {
      issueId: `controlled-${suffix}`,
      reason: { kind: 'different_removals', detailsComplete: true, changes: [], decisions: [] },
      choices: [
        {
          choiceId: 'left',
          isCurrentGroup: true,
          requiresRePairing: false,
          memberDeviceIds: members.map(m => m.deviceId),
          membersComplete: true,
          members,
          impact: {
            localDeviceOutcome: 'active',
            syncScopeDeviceIds: members.map(m => m.deviceId),
            pausedDeviceIds: [],
            pendingConfirmationDeviceIds: ['fixture-remote-0001'],
            requiresRejoinDeviceIds: [],
          },
        },
        {
          choiceId: 'right',
          isCurrentGroup: false,
          requiresRePairing: false,
          memberDeviceIds: [local],
          membersComplete: true,
          members: [members[0]],
          impact: {
            localDeviceOutcome: options.remove ? 'removed' : 'active',
            syncScopeDeviceIds: [],
            pausedDeviceIds: ['fixture-remote-0001'],
            pendingConfirmationDeviceIds: [],
            requiresRejoinDeviceIds: [],
          },
        },
      ],
    },
  ]
  if (options.unknown)
    for (const choice of groups.issues[0].choices) {
      delete choice.members
      delete choice.impact
      choice.membersComplete = false
      choice.memberDeviceIds = []
    }
  if (options.unknown) delete groups.issues[0].reason
  if (options.nullImpact) for (const choice of groups.issues[0].choices) choice.impact = null
  if (options.incomplete || options.incompleteReason)
    groups.issues[0].reason.detailsComplete = false
  if (options.incomplete || options.incompleteMembers)
    groups.issues[0].choices[0].membersComplete = false
  if (options.names)
    groups.issues[0].choices[0].members.push(
      { deviceId: 'duplicate-0002', displayName: 'Remote only', active: true, isLocal: false },
      { deviceId: 'html-0003', displayName: '<b>device</b>', active: true, isLocal: false },
      { deviceId: 'empty-0004', displayName: '', active: true, isLocal: false }
    )
  if (options.names)
    groups.issues[0].choices[0].memberDeviceIds = groups.issues[0].choices[0].members.map(
      m => m.deviceId
    )
  return groups
}

async function inject(instance, groups, outcome = 'completed') {
  await instance.execute(
    (groups, outcome) => {
      window.__conflictFixture = { groups, outcome, posts: 0, reads: 0, failRead: false }
      if (!window.__originalConflictFetch) {
        window.__originalConflictFetch = window.fetch
        window.fetch = async (input, init) => {
          const path = new URL(typeof input === 'string' ? input : input.url, location.href)
            .pathname
          if (path !== '/member/device-group-choices')
            return window.__originalConflictFetch(input, init)
          const scenario = window.__conflictFixture
          scenario.reads++
          const method = init?.method ?? input.method ?? 'GET'
          const envelope = data =>
            new Response(JSON.stringify({ data, ts: Date.now() }), {
              status: 200,
              headers: { 'Content-Type': 'application/json' },
            })
          if (method === 'POST') {
            scenario.posts++
            scenario.lastChoice = await input.clone().json()
            if (scenario.outcome === 'timeout')
              return new Promise((_, reject) => {
                const signal = init?.signal ?? input.signal
                signal.addEventListener(
                  'abort',
                  () => reject(new DOMException('Controlled request timeout', 'TimeoutError')),
                  { once: true }
                )
              })
            if (scenario.outcome === 'failure')
              return new Response(
                JSON.stringify({ code: 'controlled_failure', message: 'Controlled test failure' }),
                { status: 503, headers: { 'Content-Type': 'application/json' } }
              )
            if (scenario.outcome === 'delay')
              await new Promise(resolve => {
                scenario.finish = resolve
              })
            if (
              ['completed', 'already_completed', 'delay', 're_pairing_required'].includes(
                scenario.outcome
              )
            )
              scenario.groups = { ...scenario.groups, issues: scenario.groups.issues.slice(1) }
            if (scenario.outcome === 'state_changed') scenario.groups.revision++
            return envelope({
              outcome: scenario.outcome === 'delay' ? 'completed' : scenario.outcome,
              currentRevision: scenario.groups.revision,
            })
          }
          if (scenario.failRead)
            return new Response(JSON.stringify({ code: 'controlled_read_failure' }), {
              status: 503,
              headers: { 'Content-Type': 'application/json' },
            })
          if (scenario.holdRead) {
            scenario.holdRead = false
            const captured = structuredClone(scenario.groups)
            await new Promise(resolve => {
              scenario.releaseRead = resolve
            })
            return envelope(captured)
          }
          return envelope(scenario.groups)
        }
      }
      window.dispatchEvent(new Event('focus'))
    },
    groups,
    outcome
  )
  try {
    await (
      await instance.$('[data-testid="device-trust-confirm"]')
    ).waitForDisplayed({ timeout: 10000 })
  } catch (error) {
    console.log(
      'Controlled probe',
      await instance.execute(() => ({
        reads: window.__conflictFixture?.reads,
        visibility: document.visibilityState,
        dialogs: document.querySelectorAll('[role="dialog"]').length,
      }))
    )
    throw error
  }
}
async function clear(instance) {
  await instance.execute(() => {
    window.__conflictFixture.groups.issues = []
    window.__conflictFixture.failRead = false
    window.dispatchEvent(new Event('focus'))
  })
  await instance.waitUntil(
    async () =>
      !(await (await instance.$('[data-testid="device-trust-dialog"]')).isExisting()) ||
      (await visible(instance, '[data-testid="device-trust-done"]')),
    { timeout: 20000 }
  )
  if (await visible(instance, '[data-testid="device-trust-done"]'))
    await click(instance, '[data-testid="device-trust-done"]')
  await (await instance.$('[data-testid="device-trust-dialog"]')).waitForExist({ reverse: true })
}

export async function controlledCases(instance, conn, run) {
  const current = await request(conn, '/member/device-group-choices')
  const results = []
  for (const [id, options] of [
    ['U01', { unknown: true }],
    ['U01-null-impact', { nullImpact: true }],
    ['U02', { incomplete: true }],
    ['U02-reason-only', { incompleteReason: true }],
    ['U02-members-only', { incompleteMembers: true }],
    ['U03', {}],
    ['U04', { names: true }],
  ]) {
    await inject(instance, fixture(current, id, options))
    const confirm = await instance.$('[data-testid="device-trust-confirm"]')
    assert.equal(await confirm.isEnabled(), false)
    await screenshot(instance, run, id + '-collapsed')
    await click(instance, '[data-testid="device-trust-dialog"] button[aria-expanded="false"]')
    const text = await (await instance.$('[data-testid="device-trust-dialog"]')).getText()
    if (id.startsWith('U01')) assert.match(text, /资料暂不完整|incomplete/)
    if (id.startsWith('U02')) {
      const hasReasonWarning = /部分变化详情暂不可用|Some change details/.test(text)
      const hasMemberWarning = /可能还包含未显示的设备|may contain devices that are not shown/.test(
        text
      )
      assert.equal(hasReasonWarning, !!(options.incomplete || options.incompleteReason))
      assert.equal(hasMemberWarning, !!(options.incomplete || options.incompleteMembers))
    }
    if (id === 'U03') assert.ok(!text.includes('同步仍需确认'))
    if (id === 'U04') {
      assert.ok(text.includes('<b>device</b>'))
      assert.ok(text.includes('0001'))
      assert.ok(text.includes('0002'))
      assert.equal(
        await instance.execute(
          () => document.querySelector('[data-testid="device-trust-dialog"] b') !== null
        ),
        false
      )
    }
    await screenshot(instance, run, id + '-expanded')
    results.push({ id, passed: true, kind: 'controlled-GUI' })
    await clear(instance)
  }
  await inject(instance, fixture(current, 'waiting-for-device'), 'pending')
  await click(instance, '[data-testid="device-trust-choice-left"]')
  await click(instance, '[data-testid="device-trust-confirm"]')
  await (await instance.$('[data-testid="device-trust-recheck"]')).waitForEnabled()
  assert.match(
    await (await instance.$('[data-testid="device-trust-dialog"]')).getText(),
    /等待 Remote only 确认|Waiting for Remote only to confirm/
  )
  assert.equal(await visible(instance, '[data-testid="device-trust-done"]'), false)
  await screenshot(instance, run, 'U03-waiting-for-named-device')
  await clear(instance)
  for (const outcome of [
    'state_changed',
    'pending',
    'failure',
    'timeout',
    'local_device_confirmation_required',
    'already_completed',
    're_pairing_required',
  ]) {
    await inject(instance, fixture(current, outcome), outcome)
    await click(instance, '[data-testid="device-trust-choice-right"]')
    await click(instance, '[data-testid="device-trust-confirm"]')
    await instance.waitUntil(
      async () => await instance.execute(() => window.__conflictFixture.posts === 1)
    )
    if (outcome === 'state_changed') {
      await (await instance.$('[data-error="device_state_changed"]')).waitForDisplayed()
      assert.equal(
        await (await instance.$('[data-testid="device-trust-confirm"]')).isEnabled(),
        false
      )
      await instance.execute(() => {
        window.__conflictFixture.outcome = 'completed'
      })
      await click(instance, '[data-testid="device-trust-choice-right"]')
      await click(instance, '[data-testid="device-trust-confirm"]')
      await (await instance.$('[data-testid="device-trust-done"]')).waitForDisplayed()
      assert.equal(
        await instance.execute(
          () =>
            window.__conflictFixture.lastChoice.expectedRevision ===
            window.__conflictFixture.groups.revision
        ),
        true
      )
    } else if (outcome === 'local_device_confirmation_required') {
      await (
        await instance.$('[data-testid="device-trust-local-removal-warning"]')
      ).waitForDisplayed()
      await instance.execute(() => {
        window.__conflictFixture.outcome = 'completed'
      })
      await click(instance, '[data-testid="device-trust-confirm"]')
      assert.equal(
        await instance.execute(() => window.__conflictFixture.lastChoice.confirmLocalRemoval),
        true
      )
    } else if (outcome === 'pending' || outcome === 'failure' || outcome === 'timeout')
      await (
        await instance.$('[data-testid="device-trust-recheck"]')
      ).waitForEnabled({ timeout: 75000 })
    else await (await instance.$('[data-testid="device-trust-done"]')).waitForDisplayed()
    await screenshot(instance, run, 'U08-' + outcome)
    await clear(instance)
  }
  results.push({ id: 'U08', passed: true, kind: 'controlled-GUI' })
  await inject(instance, fixture(current, 'double-click'), 'delay')
  await click(instance, '[data-testid="device-trust-choice-left"]')
  await instance.execute(() => {
    const button = document.querySelector('[data-testid="device-trust-confirm"]')
    button.click()
    button.click()
  })
  await instance.waitUntil(
    async () => await instance.execute(() => window.__conflictFixture.posts === 1)
  )
  assert.equal(await instance.execute(() => window.__conflictFixture.posts), 1)
  await screenshot(instance, run, 'U07-submitting')
  await instance.execute(() => window.__conflictFixture.finish())
  await (await instance.$('[data-testid="device-trust-done"]')).waitForDisplayed()
  await clear(instance)
  results.push({ id: 'U07', passed: true, kind: 'controlled-GUI' })
  await inject(instance, fixture(current, 'late-read'))
  await instance.execute(() => {
    window.__conflictFixture.holdRead = true
    window.dispatchEvent(new Event('focus'))
  })
  await instance.waitUntil(async () =>
    instance.execute(() => typeof window.__conflictFixture.releaseRead === 'function')
  )
  await click(instance, '[data-testid="device-trust-choice-left"]')
  await click(instance, '[data-testid="device-trust-confirm"]')
  await (await instance.$('[data-testid="device-trust-done"]')).waitForDisplayed()
  await instance.execute(() => window.__conflictFixture.releaseRead())
  await screenshot(instance, run, 'U07-late-read-completed')
  assert.equal(await visible(instance, '[data-testid="device-trust-done"]'), true)
  assert.equal(await instance.execute(() => window.__conflictFixture.posts), 1)
  await clear(instance)
  for (const language of ['zh-CN', 'en-US'])
    for (const theme of ['light', 'dark']) {
      await request(conn, '/settings', 'PUT', { general: { language, theme } })
      await instance.execute(
        language => localStorage.setItem('uniclipboard.language', language),
        language
      )
      await instance.execute(() => {
        window.__beforeControlledReload = true
      })
      await instance.refresh()
      await instance.waitUntil(
        async () =>
          instance.execute(
            () =>
              !window.__beforeControlledReload &&
              !!document.querySelector('[data-testid="history-preview-motion"]') &&
              !document.querySelector('[data-testid="device-trust-dialog"]')
          ),
        { timeout: 30000, timeoutMsg: 'reloaded application did not become ready' }
      )
      for (const [width, height] of [
        [1100, 800],
        [720, 600],
      ]) {
        await instance.execute(
          (width, height) => {
            window.__resizeError = null
            void window.__TAURI_INTERNALS__
              .invoke('plugin:window|set_min_size', { label: 'main', value: null })
              .then(() =>
                window.__TAURI_INTERNALS__.invoke('plugin:window|set_size', {
                  label: 'main',
                  value: { Logical: { width, height } },
                })
              )
              .catch(error => {
                window.__resizeError = String(error)
              })
          },
          width,
          height
        )
        await instance.waitUntil(
          async () =>
            instance.execute(
              (width, height) => {
                if (window.__resizeError) throw new Error(window.__resizeError)
                return Math.abs(innerWidth - width) < 3 && Math.abs(innerHeight - height) < 60
              },
              width,
              height
            ),
          { timeout: 10000, timeoutMsg: 'window dimensions did not update' }
        )
        await inject(instance, fixture(current, `${language}-${theme}-${width}`, { long: true }))
        await click(instance, '[data-testid="device-trust-dialog"] button[aria-expanded="false"]')
        const bounds = await instance.execute(() => {
          const dialog = document.querySelector('[data-testid="device-trust-dialog"]')
          const footer = document.querySelector('[data-slot="dialog-footer"]')
          const b = footer.getBoundingClientRect(),
            d = dialog.getBoundingClientRect()
          return {
            footerVisible: b.top >= 0 && b.bottom <= innerHeight,
            contained: d.left >= 0 && d.right <= innerWidth,
            noOverflow: dialog.scrollWidth <= dialog.clientWidth + 1,
          }
        })
        assert.deepEqual(bounds, { footerVisible: true, contained: true, noOverflow: true })
        await screenshot(instance, run, `U05-${language}-${theme}-${width}`)
        await clear(instance)
      }
    }
  results.push({ id: 'U05', passed: true, kind: 'controlled-GUI' })
  await inject(instance, fixture(current, 'keyboard'))
  const routeBefore = await instance.getUrl()
  await instance.action('pointer').move({ x: 15, y: 100 }).down().up().perform()
  assert.equal(await visible(instance, '[data-testid="device-trust-dialog"]'), true)
  assert.equal(await instance.getUrl(), routeBefore)
  await instance.keys(['Escape'])
  assert.equal(await visible(instance, '[data-testid="device-trust-dialog"]'), true)
  await instance.execute(() => document.querySelector('[role="radio"]').focus())
  await instance.keys(['ArrowDown'])
  assert.equal(
    await (
      await instance.$('[data-testid="device-trust-choice-right"]')
    ).getAttribute('aria-checked'),
    'true'
  )
  await instance.keys(['Tab'])
  await instance.waitUntil(
    async () =>
      instance.execute(() =>
        document
          .querySelector('[data-testid="device-trust-dialog"]')
          .contains(document.activeElement)
      ),
    { timeout: 3000, timeoutMsg: 'keyboard focus escaped the modal' }
  )
  await screenshot(instance, run, 'U06-keyboard')
  await clear(instance)
  results.push({ id: 'U06', passed: true, kind: 'controlled-GUI' })
  const multi = fixture(current, 'first')
  multi.issues.push(fixture(current, 'second', { remove: true }).issues[0])
  await inject(instance, multi)
  await instance.execute(() => {
    window.__originalChoiceDialog = document.querySelector('[data-testid="device-trust-dialog"]')
  })
  await click(instance, '[data-testid="device-trust-choice-left"]')
  await click(instance, '[data-testid="device-trust-confirm"]')
  await (await instance.$('[data-testid="device-trust-done"]')).waitForDisplayed()
  assert.equal(
    await instance.execute(
      () =>
        window.__originalChoiceDialog ===
        document.querySelector('[data-testid="device-trust-dialog"]')
    ),
    true
  )
  await screenshot(instance, run, 'U10-first-completed')
  await click(instance, '[data-testid="device-trust-done"]')
  await (await instance.$('[data-testid="device-trust-confirm"]')).waitForDisplayed()
  assert.equal(
    await instance.execute(
      () =>
        window.__originalChoiceDialog ===
        document.querySelector('[data-testid="device-trust-dialog"]')
    ),
    true
  )
  assert.equal(await (await instance.$('[data-testid="device-trust-confirm"]')).isEnabled(), false)
  await click(instance, '[data-testid="device-trust-choice-right"]')
  await click(instance, '[data-testid="device-trust-confirm"]')
  await (await instance.$('[data-testid="device-trust-local-removal-warning"]')).waitForDisplayed()
  await screenshot(instance, run, 'U10-second-confirmation')
  await clear(instance)
  results.push({ id: 'U10', passed: true, kind: 'controlled-GUI' })
  await inject(instance, fixture(current, 'updated-details'))
  await click(instance, '[data-testid="device-trust-choice-left"]')
  await click(instance, '[data-testid="device-trust-dialog"] button[aria-expanded="false"]')
  await instance.execute(() => {
    window.__conflictFixture.groups.revision++
    window.__conflictFixture.groups.issues[0].choices[0].members[1].displayName = 'Updated name'
    window.dispatchEvent(new Event('focus'))
  })
  await instance.waitUntil(async () =>
    (await (await instance.$('[data-testid="device-trust-dialog"]')).getText()).includes(
      'Updated name'
    )
  )
  assert.equal(
    await (
      await instance.$('[data-testid="device-trust-choice-left"]')
    ).getAttribute('aria-checked'),
    'true'
  )
  await instance.execute(() => {
    window.__conflictFixture.groups.revision++
    window.__conflictFixture.groups.issues[0].choices[0].impact.pausedDeviceIds = [
      'fixture-remote-0001',
    ]
    window.dispatchEvent(new Event('focus'))
  })
  await instance.waitUntil(
    async () => !(await (await instance.$('[data-testid="device-trust-confirm"]')).isEnabled())
  )
  assert.equal(await instance.execute(() => window.__conflictFixture.posts), 0)
  await screenshot(instance, run, 'U10-changed-impact-requires-selection')
  await clear(instance)
  await inject(instance, fixture(current, 'query-failure'))
  await instance.execute(() => {
    window.__conflictFixture.failRead = true
    window.dispatchEvent(new Event('focus'))
  })
  await (await instance.$('[data-testid="device-trust-error"]')).waitForDisplayed()
  assert.equal(await visible(instance, '[data-testid="device-trust-dialog"]'), true)
  await screenshot(instance, run, 'U09-read-failure')
  await clear(instance)
  results.push({
    id: 'U09',
    passed: true,
    kind: 'controlled-GUI',
    note: 'query failure, submission failure, and real request deadline expiry',
  })
  await instance.execute(() => {
    window.__conflictFixture.failRead = true
    window.dispatchEvent(new Event('focus'))
  })
  await (await instance.$('[data-testid="device-trust-error"]')).waitForDisplayed()
  await screenshot(instance, run, 'U09-refresh-fails-after-completion')
  await instance.execute(() => {
    window.__conflictFixture.failRead = false
  })
  await click(instance, '[data-testid="device-trust-recheck"]')
  await (await instance.$('[data-testid="device-trust-dialog"]')).waitForExist({ reverse: true })
  await instance.execute(() => {
    window.fetch = window.__originalConflictFetch
    delete window.__originalConflictFetch
    delete window.__conflictFixture
    window.dispatchEvent(new Event('focus'))
  })
  await writeFile(join(run.runDir, 'controlled-results.json'), JSON.stringify(results, null, 2))
}

export async function nativeKeyboardCase(instance, conn, run) {
  assertNativeKeyboardAvailable()
  const windowError = await instance.executeAsync(done => {
    window.__TAURI_INTERNALS__
      .invoke('plugin:window|show', { label: 'main' })
      .then(() => window.__TAURI_INTERNALS__.invoke('plugin:window|set_focus', { label: 'main' }))
      .then(
        () => done(null),
        error => done(String(error))
      )
  })
  assert.equal(windowError, null, 'native test window must be visible and focused')
  const current = await request(conn, '/member/device-group-choices')
  await instance.waitUntil(
    async () =>
      instance.execute(
        () =>
          !!document.querySelector('[data-testid="history-preview-motion"]') &&
          !document.querySelector('[data-testid="device-trust-dialog"]')
      ),
    { timeout: 30000, timeoutMsg: 'main window did not become ready for keyboard testing' }
  )
  await instance.execute(() => {
    const target = [...document.querySelectorAll('button:not(:disabled)')].find(
      button => button.getClientRects().length > 0
    )
    if (!target) throw new Error('No main-window focus target')
    target.focus()
    window.__beforeChoiceFocus = target
  })
  await inject(instance, fixture(current, 'native-keyboard'))
  await instance.execute(() => {
    window.__nativeKeys = []
    window.addEventListener(
      'keydown',
      event => {
        if (['Escape', 'Tab', ' ', 'Enter'].includes(event.key))
          window.__nativeKeys.push({
            key: event.key,
            trusted: event.isTrusted,
            target: event.target?.getAttribute?.('data-testid') ?? event.target?.tagName,
          })
      },
      { capture: true }
    )
    document.querySelector('[role="radio"]').focus()
  })
  await screenshot(instance, run, 'U06-native-before')
  const pids = execFileSync('pgrep', ['-x', 'uniclipboard'], { encoding: 'utf8' })
    .trim()
    .split(/\s+/)
  const logPrefix = `n${join(homedir(), 'Library/Logs', `app.uniclipboard.desktop-${conn.profile}`)}/`
  const owners = pids.filter(pid =>
    execFileSync('lsof', ['-p', pid, '-Fn'], { encoding: 'utf8' })
      .split('\n')
      .some(line => line.startsWith(logPrefix))
  )
  assert.equal(owners.length, 1, 'native input must target exactly one test process')
  const pid = Number(owners[0])
  assert.equal(
    execFileSync('ps', ['-p', String(pid), '-o', 'command='], { encoding: 'utf8' }).trim(),
    resolve('target/debug/uniclipboard')
  )
  const keysToOS = codes => {
    assertNativeKeyboardAvailable()
    return execFileSync(
      'osascript',
      [
        '-e',
        `tell application "System Events" to tell (first application process whose unix id is ${pid})`,
        '-e',
        'set frontmost to true',
        ...codes.flatMap(code => ['-e', `key code ${code}`]),
        '-e',
        'end tell',
      ],
      { stdio: 'pipe' }
    )
  }
  keysToOS([])
  assert.equal(
    await instance.executeAsync(done => {
      window.__TAURI_INTERNALS__.invoke('plugin:window|set_focus', { label: 'main' }).then(
        () => done(null),
        error => done(String(error))
      )
    }),
    null
  )
  await instance.execute(() =>
    document.querySelector('[data-testid="device-trust-choice-left"]').focus()
  )
  keysToOS([53])
  await instance.waitUntil(async () =>
    instance.execute(() =>
      window.__nativeKeys.some(event => event.key === 'Escape' && event.trusted)
    )
  )
  assert.equal(await visible(instance, '[data-testid="device-trust-dialog"]'), true)
  assert.equal(
    await instance.execute(() =>
      document.querySelector('[data-testid="device-trust-dialog"]').contains(document.activeElement)
    ),
    true
  )
  keysToOS([49])
  await instance.waitUntil(
    async () =>
      (await (
        await instance.$('[data-testid="device-trust-choice-left"]')
      ).getAttribute('aria-checked')) === 'true'
  )
  await screenshot(instance, run, 'U06-native-space-selected')
  keysToOS([48, 48])
  await instance.waitUntil(
    async () =>
      instance.execute(
        () => document.activeElement?.getAttribute('data-testid') === 'device-trust-confirm'
      ),
    { timeout: 3000 }
  )
  keysToOS([36])
  await (await instance.$('[data-testid="device-trust-done"]')).waitForDisplayed()
  await screenshot(instance, run, 'U06-native-enter-result')
  keysToOS([48])
  await instance.waitUntil(
    async () =>
      instance.execute(
        () => document.activeElement?.getAttribute('data-testid') === 'device-trust-done'
      ),
    { timeout: 3000 }
  )
  keysToOS([36])
  await (await instance.$('[data-testid="device-trust-dialog"]')).waitForExist({ reverse: true })
  await instance.waitUntil(
    async () => instance.execute(() => document.activeElement === window.__beforeChoiceFocus),
    { timeout: 3000, timeoutMsg: 'main-window keyboard focus was not restored' }
  )
  const keys = await instance.execute(() => window.__nativeKeys)
  await writeFile(join(run.runDir, 'native-key-events.json'), JSON.stringify(keys))
  for (const key of ['Escape', 'Tab', ' ', 'Enter'])
    assert.ok(
      keys.some(event => event.key === key && event.trusted),
      `native ${key} missing`
    )
  await screenshot(instance, run, 'U06-native-completed')
  await writeFile(
    join(run.runDir, 'native-keyboard-result.json'),
    JSON.stringify({
      passed: true,
      keys,
      note: 'actual OS keyboard input, controlled backend response',
    })
  )
}
