import * as HostService from '@host/hostservice'
import {
  DownloadEventKind,
  DownloadPhase,
  EffectsMode,
  InstallKind,
  QuickPanelDoubleTapModifier,
  type DownloadEvent,
  type EffectsSnapshot,
} from '@host/models'
// E2E-only scenario driver. It runs inside the real Wails WebView, interacts
// with the shared React DOM and reports each assertion to the native test
// service. It is bundled only when VITE_GUI_GO_E2E=1.
import { Call, Events, Window } from '@wailsio/runtime'
import { lockEncryption } from '@/api/daemon'
import { daemonClient } from '@/api/daemon/client'
import { getSettings, updateSettings } from '@/api/daemon/settings'
import { setQuickPanelEnabled, setQuickPanelPosition } from '@/api/tauri-command/settings'
import { expectedSkin, modifier, settingsPageOpen, skinClass } from '@/e2e/skin'
import { isPermissionGranted, requestPermission, sendNotification } from '@/host/notification'
import { subscribeWindowMaterial } from '@/host/window-material'
import i18n from '@/i18n'
import { daemonWs } from '@/lib/daemon-ws'
import { commands } from '@/lib/ipc'
import { setUiScale } from '@/lib/ui-scale'
import { isExpectedCommandError } from '@/observability/errors'

const windowName = 'main'
// Keep recent console errors so a crashed UI reports its cause, not just a timeout.
const consoleErrors: string[] = []
const nativeConsoleError = console.error.bind(console)
console.error = (...args: unknown[]) => {
  consoleErrors.push(
    args
      .map(a => (a instanceof Error ? (a.stack ?? a.message) : String(a)))
      .join(' ')
      .slice(0, 400)
  )
  nativeConsoleError(...args)
}
const SETUP_PASSPHRASE = 'gui-go-synthetic-passphrase'

const record = (step: string, ok: boolean, detail?: unknown) =>
  Call.ByName('main.EvidenceService.Record', {
    window: windowName,
    step,
    ok,
    detail,
  })
const control = (action: string) => Call.ByName('main.EvidenceService.Control', action)
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))
const $ = (selector: string) => document.querySelector<HTMLElement>(selector)

async function waitFor<T>(label: string, probe: () => T | null | undefined | false, ms = 60000) {
  const end = Date.now() + ms
  while (Date.now() < end) {
    const value = probe()
    if (value) return value
    await sleep(100)
  }
  throw new Error(`timeout: ${label}`)
}

function fill(selector: string, value: string) {
  const input = $(selector) as HTMLInputElement
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
  setter.call(input, value)
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

const link = (href: string) => $(`a[href="${href}"]`)
// The History page built on `@/ui-kit` (macOS and Linux builds) navigates with sidebar buttons, not links.
const kitNavLabels: Record<string, string> = {
  '/settings': 'history.kit.settings',
  '/devices': 'history.kit.pairDevice',
}
const navControl = (href: string) =>
  link(href) ??
  // The row text may end with a shortcut hint (`Settings… ⌘,`).
  Array.from(document.querySelectorAll<HTMLElement>('nav button')).find(
    button =>
      kitNavLabels[href] && button.textContent?.trim().startsWith(i18n.t(kitNavLabels[href]))
  ) ??
  null
const devicesPage = () => $('[data-testid="devices-add-device"]')
// History rows of either page: the existing cards, or the `@/ui-kit` list's options.
const ROW_SELECTOR = '[data-testid="history-card"], [role="option"][data-item-id]'
const historyRows = () => Array.from(document.querySelectorAll<HTMLElement>(ROW_SELECTOR))
const historyRow = (id: string) =>
  $(`[data-testid="history-card"][data-entry-id="${id}"], [role="option"][data-item-id="${id}"]`)
// Opens a row the way a pointer does on that page.
function openRow(row: HTMLElement) {
  const button = row.dataset.testid === 'history-card' ? row.querySelector('button') : null
  if (button) button.click()
  else row.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }))
}
// The history route with the sidebar shell: independent of list content and locale.
const mainLayout = () =>
  location.pathname.startsWith('/history') &&
  navControl('/settings') &&
  !$('[data-testid="setup-entry-create"]') &&
  !$('#unlock-passphrase') &&
  !$('[data-testid="unlock-content"]')

async function navigate(
  go: () => void,
  href: string,
  marker: () => unknown,
  step: string,
  retryClick = false,
  retryOnceWhenStuck = false
) {
  go()
  const arrived = () => location.pathname.startsWith(href) && marker()
  // A click can land while the sidebar is still animating in; retry link clicks until the route changes.
  for (let attempt = 0; retryClick && attempt < 20 && !arrived(); attempt++) {
    await sleep(1000)
    if (!arrived()) go()
  }
  // A route change that has not happened after a few seconds is recorded with the facts that decide why (history depth,
  // visibility, focus), then the wait below still fails the step on a timeout.
  for (let waited = 0; waited < 5000 && !arrived(); waited += 250) await sleep(250)
  if (!arrived()) {
    await record(`${step}-slow`, true, {
      path: location.pathname,
      expected: href,
      historyLength: history.length,
      visibility: document.visibilityState,
      hasFocus: document.hasFocus(),
    })
    // One more attempt for a request that was dropped (a session-history traversal issued right after a navigation can be);
    // nothing has moved, so it cannot traverse twice. Whether it helped is recorded.
    if (retryOnceWhenStuck) {
      go()
      for (let waited = 0; waited < 5000 && !arrived(); waited += 250) await sleep(250)
      await record(`${step}-retried`, !!arrived(), {
        path: location.pathname,
        historyLength: history.length,
      })
    }
  }
  await waitFor(step, arrived)
  await record(step, true, { path: location.pathname })
}

async function run() {
  const phase = (await Call.ByName('main.EvidenceService.Phase')) as string
  if (phase === 'linux-package-update') return runLinuxPackageUpdateScenario()
  if (phase === 'host-contract') return runHostContractScenario()
  if (phase === 'download-cancel') return runDownloadCancelScenario()
  if (phase.startsWith('update')) return runUpdateScenario(phase)
  if (phase === 'file-preview') return runFilePreviewScenario()
  if (phase === 'key-path-verify') {
    await waitFor('app root content', () => document.getElementById('root')?.children.length)
    await control('update-verify')
    return control('exit')
  }
  if (phase === 'unlock-wrong') return runUnlockWrongScenario()
  if (phase === 'unlock-restart') return runUnlockRestartScenario()
  if (phase === 'history-live') return runHistoryLiveScenario()
  if (phase === 'history-kit') return runHistoryKitScenario()
  if (phase.startsWith('linux-kit-')) {
    // Linux kit flows (e2e/linux-kit-flows.ts): the kit Settings page.
    const flows = await import('@/e2e/linux-kit-flows')
    const ctx = {
      record,
      control,
      waitFor,
      sleep,
      $,
      fill,
      navControl,
      mainLayout,
      reachMainLayout,
    }
    if (phase === 'linux-kit-settings') return flows.runSettingsKitScenario(ctx)
    if (phase === 'linux-kit-settings-restart') return flows.runSettingsRestartScenario(ctx)
  }
  if (phase === 'sidebar-toggle') {
    await reachMainLayout()
    return (await import('@/e2e/sidebar-toggle')).runSidebarToggleScenario(record, control)
  }
  if (phase === 'single-image-ui') return runSingleImageUiScenario()
  if (phase === 'scheduler') return runSchedulerScenario()
  if (phase === 'wake') return // the orchestrator drives this launch through the control file
  if (phase === 'external') return // an external CDP driver owns this launch; the default scenario would race it
  if (phase === 'entry-cdp') {
    // CDP owns all input; snapshots only observe, and locking uses the real daemon API.
    Object.defineProperty(window, '__ucEntryLockContent', {
      value: lockEncryption,
    })
    Object.defineProperty(window, '__ucEntrySnapshot', {
      value: async () => ({
        skin: $('[data-ui-skin]')?.dataset.uiSkin,
        path: location.pathname,
        language: i18n.language,
        main: !!mainLayout(),
        unlocked: await contentUnlocked(),
        setup: !!$('[data-testid="setup-entry-create"]'),
        initialize: !!$('#device-name') && !!$('#pass1') && !!$('#pass2'),
        unlock: !!$('[data-testid="unlock-content"]') || !!$('#unlock-passphrase'),
        recovery: !!$('#recovery-passphrase'),
        passphrase: !!$('#unlock-passphrase'),
        passphraseInvalid: $('#unlock-passphrase')?.getAttribute('aria-invalid'),
        alerts: Array.from(document.querySelectorAll('[role="alert"]')).map(node =>
          node.textContent?.trim()
        ),
        wrongPassphrase: i18n.t('unlock.errors.wrongPassphrase'),
      }),
    })
    return
  }
  if (phase.startsWith('diag:')) {
    const { runDiagnosticsAcceptance } = await import('@/e2e/diagnostics-acceptance')
    return runDiagnosticsAcceptance(phase, {
      record,
      control,
      waitFor,
      sleep,
      reachMainLayout,
      navControl,
    })
  }
  if (phase === 'quick-panel-settings') return runQuickPanelSettingsScenario()
  if (phase === 'native-panel') return runNativePanelScenario()
  if (phase === 'file-ops') return runFileOpsScenario()
  if (phase === 'startup-observe') return // the native observer reports; the hidden window may not run scripts
  if (phase.startsWith('startup-set:')) return runStartupSetScenario(phase)
  if (phase === 'tray-devices') return runTrayDevicesScenario()
  if (phase === 'device-group-update') return runDeviceGroupUpdateScenario()
  if (phase.startsWith('autostart')) return runAutostartScenario(phase)
  if (phase === 'config-export') return runConfigExportScenario()
  if (phase === 'config-applied') {
    await waitFor('app root content', () => document.getElementById('root')?.children.length)
    await control('prefs')
    return control('exit')
  }
  if (phase === 'native-requests') return runNativeRequestsScenario()
  if (phase.startsWith('linux-shortcut-ui')) return runLinuxShortcutUiScenario(phase)
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  // A diagnostic page would never contain the shared app's router state.
  await record('shared-app-mounted', !$('#refresh') && !!document.getElementById('root'))

  let snapshot = false
  const unsubscribe = daemonWs.subscribe(['status'], event => {
    if (event.eventType === 'status.snapshot') snapshot = true
  })
  await waitFor('ws status snapshot', () => snapshot)
  unsubscribe()
  await record('ws-status-snapshot', true)

  const state = await waitFor('first screen', () =>
    $('[data-testid="setup-entry-create"]')
      ? 'setup'
      : $('#unlock-passphrase') || $('[data-testid="unlock-content"]')
        ? 'locked'
        : mainLayout()
          ? 'home'
          : null
  )
  await record('first-screen', true, { state })

  if (state === 'setup') {
    $('[data-testid="setup-entry-create"]')!.click()
    await waitFor('device form', () => $('#device-name'))
    fill('#device-name', 'gui-go-synthetic')
    fill('#pass1', SETUP_PASSPHRASE)
    fill('#pass2', SETUP_PASSPHRASE)
    await sleep(200)
    $('[data-testid="setup-initialize-submit"]')!.click()
    await waitFor('setup complete', () => $('[data-testid="setup-complete-later"]') || mainLayout())
    await record('setup-created-in-gui', true)
    $('[data-testid="setup-complete-later"]')?.click()
  } else if (state === 'locked') {
    if (!$('#unlock-passphrase')) $('[data-testid="unlock-content"]')?.click()
    // The keyring may unlock directly; otherwise the passphrase form follows.
    const next = await waitFor('unlock outcome', () =>
      $('#unlock-passphrase')
        ? 'passphrase'
        : $('[data-testid="unlock-content"]')
          ? null
          : 'keyring'
    )
    if (next === 'passphrase') {
      fill('#unlock-passphrase', `${SETUP_PASSPHRASE}-wrong`)
      ;($('#unlock-passphrase') as HTMLInputElement).form?.requestSubmit()
      await waitFor(
        'wrong passphrase message',
        () =>
          document.body.innerText.includes('口令错误') ||
          document.body.innerText.includes(i18n.t('unlock.errors.wrongPassphrase'))
      )
      await record('unlock-wrong-passphrase-rejected', true)
      fill('#unlock-passphrase', SETUP_PASSPHRASE)
      ;($('#unlock-passphrase') as HTMLInputElement).form?.requestSubmit()
    }
    await waitFor('unlocked', () => mainLayout())
    await record('unlock-accepted', true, { via: next })
  }

  await waitFor('home', mainLayout)
  await record('home', true)
  if (phase === 'startup-frames') {
    const observed = window as unknown as {
      __UC_STARTUP_FRAMES__: Array<{ skin: string; phase: string; at: number }>
      __UC_STOP_STARTUP_FRAMES__: () => void
    }
    observed.__UC_STOP_STARTUP_FRAMES__()
    const frames = observed.__UC_STARTUP_FRAMES__
    await record(
      'startup-frames',
      frames.length > 0 && frames.every(frame => frame.skin === expectedSkin()),
      frames
    )
    await sleep(1500)
    await record('driver-complete', true)
    return control('exit')
  }
  const click = (href: string) => () =>
    void waitFor(`link ${href}`, () => navControl(href)).then(a => a.click())
  // At this window width the shared layout shows devices as a panel of the history route.
  await navigate(click('/devices'), '/', devicesPage, 'devices', true)
  const beforeSettings = location.pathname
  await navigate(click('/settings'), '/settings', () => settingsPageOpen(), 'settings', true)
  await navigate(
    () => history.back(),
    beforeSettings,
    () => !settingsPageOpen(),
    'navigate-back',
    false,
    true
  )

  await control('close-main')
  await control('reopen-main')
  // The WebView survived the hide/show cycle with its React state and host bindings intact.
  const alive = await settle(HostService.GetDeviceMeta())
  await record('webview-alive-after-reopen', alive.status === 'ok' && !!navControl('/settings'))
  // Second windows: the real updater (dev preview) and quick panel pages.
  await control('open-updater')
  await sleep(3000)
  await control('close-updater')
  await control('show-quick-panel')
  await sleep(3000)
  await control('dismiss-quick-panel')
  const refusals = await checkPreviewRefusals()
  await record(
    'file-preview-refusals',
    Object.values(refusals).every(code => code === 404),
    refusals
  )
  await control('tray-check')
  await record('driver-complete', true)
  // Give the orchestrator time to read daemon state before the GUI exits.
  await sleep(2500)
  await control('exit')
}

// The preview route must refuse anything the daemon's history does not reference.
async function checkPreviewRefusals(): Promise<Record<string, number>> {
  const probes: Record<string, string> = {
    notAnImage: '/etc/passwd',
    unknownImagePath: '/etc/not-in-history.png',
    traversal: '/tmp/../etc/not-in-history.png',
    relative: 'not-in-history.png',
  }
  const statuses: Record<string, number> = {}
  for (const [name, path] of Object.entries(probes)) {
    statuses[name] = (await fetch(`/host-file?path=${encodeURIComponent(path)}`)).status
  }
  return statuses
}

// `file://` URIs of file entries as the daemon reports them (the shared frontend's path source).
async function historyFileURIs(): Promise<string[]> {
  const end = Date.now() + 90000
  while (Date.now() < end) {
    const response = await daemonClient.request<{
      data: Array<{ preview: string }>
    }>('/clipboard/entries?limit=50&offset=0')
    const found = response.data
      .flatMap(entry => entry.preview.split('\n'))
      .filter(line => /^file:\/\//i.test(line.trim()))
    if (found.length > 0) return found
    await sleep(1000)
  }
  throw new Error('timeout: file entry in history')
}

// Brings a CLI-created profile to the main layout the way a user would: dismiss a leftover setup step,
// unlock through the keyring.
async function reachMainLayout() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  // CLI-created spaces can leave the setup flow on its invitation step; dismiss it like a user would.
  await waitFor('unlock or home', () => {
    $('[data-testid="setup-invitation-cancel"]')?.click()
    $('[data-testid="setup-complete-later"]')?.click()
    return $('[data-testid="unlock-content"]') || mainLayout()
  })
  if ($('[data-testid="unlock-content"]')) {
    $('[data-testid="unlock-content"]')!.click()
    await waitFor('unlocked', () => mainLayout())
  }
}

// A received image file must preview through the host route; a locked or unknown path must not.
async function runFilePreviewScenario() {
  await reachMainLayout()
  const refusals = await checkPreviewRefusals()
  await record(
    'file-preview-refusals',
    Object.values(refusals).every(code => code === 404),
    refusals
  )
  // Take the received file's path from the daemon's history, exactly where the shared frontend
  // reads it, and fetch it through the host route.
  const entries = await historyFileURIs()
  const path = decodeURIComponent(new URL(entries[0].trim()).pathname)
  const response = await fetch(`/host-file?path=${encodeURIComponent(path)}`)
  const blob = await response.blob()
  const bitmap = await createImageBitmap(blob)
  await record('file-preview-image-loaded', response.status === 200 && bitmap.width > 0, {
    status: response.status,
    type: response.headers.get('content-type'),
    csp: response.headers.get('content-security-policy'),
    width: bitmap.width,
    path: path.replace(/^.*\/iroh-blobs\//, '.../iroh-blobs/'),
  })
  await sleep(1500)
  await control('exit')
}

// The generated binding, called without the shared frontend's wrapper: what the Go service answers.
const contentUnlocked = () => HostService.GetContentUnlocked()

// A call settled into the shape the older scenarios assert on: `ok` with the data, or `error` with the
// payload the host marshalled (Wails puts it in the RuntimeError's `cause`).
async function settle<T>(call: Promise<T>) {
  try {
    return { status: 'ok' as const, data: await call }
  } catch (error) {
    const cause = (error as { cause?: unknown }).cause
    return {
      status: 'error' as const,
      error: cause ?? error,
      raw: { name: (error as Error).name, hasCause: cause != null },
    }
  }
}

// Two ways a profile ends up asking for the passphrase, both through the shared pages: the profile
// recovery page (the master key is gone) and the unlock page's passphrase form (the keyring unlock fails).
// A wrong passphrase must be refused with the localized message and leave content locked; the right one unlocks.
async function runUnlockWrongScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const secret = String(await Call.ByName('main.EvidenceService.Secret'))
  const screen = await waitFor('passphrase screen', () =>
    $('#recovery-passphrase') ? 'recovery' : $('[data-testid="unlock-content"]') ? 'unlock' : null
  )
  await record('locked-screen', true, {
    screen,
    unlocked: await contentUnlocked(),
  })
  if (screen === 'unlock') {
    $('[data-testid="unlock-content"]')!.click()
    await waitFor('passphrase form after the keyring attempt', () => $('#unlock-passphrase'))
  }
  await probeUnlockContract(secret)
  const input = screen === 'recovery' ? '#recovery-passphrase' : '#unlock-passphrase'
  const submit = () => ($(input) as HTMLInputElement).form?.requestSubmit()
  let lockEvents = 0
  const offLock = Events.On('content-lock-changed', () => void lockEvents++)
  fill(input, `${secret}-wrong`)
  submit()
  const alert = await waitFor('wrong passphrase alert', () => $('[role="alert"]'))
  const wanted = i18n.t('unlock.errors.wrongPassphrase')
  await record('wrong-passphrase-rejected', alert.textContent?.trim() === wanted && !!wanted, {
    shown: alert.textContent?.trim(),
    wanted,
  })
  await record(
    'still-locked-after-wrong-passphrase',
    (await contentUnlocked()) === false && !mainLayout()
  )
  fill(input, secret)
  submit()
  await waitFor('unlocked', () => mainLayout())
  await record('right-passphrase-unlocked', (await contentUnlocked()) === true)
  await waitFor('content-lock-changed event', () => lockEvents > 0)
  offLock()
  await record('content-lock-changed-event', lockEvents > 0, {
    events: lockEvents,
  })
  await sleep(1500)
  await control('exit')
}

// The unlock command end to end through the generated binding, on a still locked profile: the typed business
// rejection (the stable code, user-facing, never reported), the same call through the shared frontend wrapper, and
// the two failures that are NOT business errors (a call the framework itself rejects, an unknown host failure).
async function probeUnlockContract(secret: string) {
  const wrong = `${secret}-probe-wrong`
  const direct = await settle(HostService.UnlockContent({ passphrase: wrong }))
  await record(
    'binding-wrong-passphrase-typed',
    direct.status === 'error' &&
      (direct.error as { code?: string }).code === 'WRONG_PASSPHRASE' &&
      direct.raw.name === 'RuntimeError' &&
      direct.raw.hasCause &&
      isExpectedCommandError(direct.error),
    { observed: direct }
  )
  const viaWrapper = await settle(commands.unlockContent({ passphrase: wrong }))
  await record(
    'wrapper-wrong-passphrase-user-facing',
    viaWrapper.status === 'error' &&
      (viaWrapper.error as { code?: string }).code === 'WRONG_PASSPHRASE' &&
      isExpectedCommandError(viaWrapper.error),
    { observed: viaWrapper }
  )
  // Wrong argument count (a call that bypasses the generated function): the framework rejects before the
  // method runs. No payload, so it stays a plain Error and counts as a system error.
  const malformed = await settle(Call.ByName('main.HostService.UnlockContent'))
  await record(
    'binding-malformed-call-is-system-error',
    malformed.status === 'error' &&
      malformed.raw.name === 'TypeError' &&
      !isExpectedCommandError(malformed.error),
    { observed: malformed }
  )
  // `null` where a value type is expected is not an error for Wails: it decodes to the zero value and the method
  // runs (here an empty passphrase, which the daemon refuses like any wrong one). The generated signature is what
  // keeps `undefined` and `null` out; the contract document lists this for every required parameter.
  const nullArg = await settle(
    (HostService.UnlockContent as unknown as (v: null) => Promise<void>)(null)
  )
  await record(
    'binding-null-argument-decodes-to-zero-value',
    nullArg.status === 'error' && (nullArg.error as { code?: string }).code === 'WRONG_PASSPHRASE',
    { observed: nullArg }
  )
  await record('binding-still-locked-after-probes', (await contentUnlocked()) === false)
}

// After a recovery the next launch must unlock through the keyring again (the recovered key was stored back)
// and the history written before the key loss must be readable.
async function runUnlockRestartScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const marker = String(await Call.ByName('main.EvidenceService.Secret'))
  const screen = await waitFor('first screen', () =>
    $('#recovery-passphrase')
      ? 'recovery'
      : $('#unlock-passphrase')
        ? 'passphrase'
        : $('[data-testid="unlock-content"]')
          ? 'unlock'
          : mainLayout()
            ? 'home'
            : null
  )
  await record('restart-first-screen', screen === 'unlock' || screen === 'home', { screen })
  if (screen === 'unlock') $('[data-testid="unlock-content"]')!.click()
  await waitFor('unlocked by the keyring', () => mainLayout())
  const response = await daemonClient.request<{
    data: Array<{ preview: string }>
  }>('/clipboard/entries?limit=50&offset=0')
  const restored = response.data.some(entry => entry.preview.includes(marker))
  await record('history-restored-after-recovery', restored, {
    entries: response.data.length,
  })
  await sleep(1000)
  await control('exit')
}

async function runHistoryLiveScenario() {
  const marker = String(await Call.ByName('main.EvidenceService.Secret'))
  await reachMainLayout()
  const cards = historyRows
  const withMarker = () => cards().find(card => card.textContent?.includes(marker))
  // Every raw frame the socket delivers after arming, whatever the topic, so the evidence names what drove the update.
  const socket = daemonWs as unknown as {
    _handleMessage: (data: string) => void
  }
  const handle = socket._handleMessage.bind(daemonWs)
  let armed = false
  const frames: string[] = []
  socket._handleMessage = (data: string) => {
    if (armed) {
      try {
        const m = JSON.parse(data)
        frames.push(`${m.topic}:${m.type ?? m.event_type}`)
      } catch {
        frames.push('unparsed')
      }
    }
    handle(data)
  }
  // Same document, same route for the whole scenario: a reload or navigation would make the check meaningless.
  const loadId = Math.random().toString(36).slice(2)
  ;(window as unknown as { __e2eLoadId: string }).__e2eLoadId = loadId
  const route = location.pathname
  await waitFor('history list rendered', () => $(ROW_SELECTOR)) // the orchestrator seeds one entry
  await sleep(1500) // let the initial fetch settle before arming
  armed = true // before the step is reported: the orchestrator injects the entry once it sees the step
  await record('history-watch-armed', !withMarker(), {
    cardsBefore: cards().length,
    route,
  })
  const armedAt = Date.now()
  const card = await waitFor('history card with the sent text', withMarker, 90000)
  const cardAfterMs = Date.now() - armedAt
  // The page learns about daemon writes over the WebSocket: a clipboard or search frame must accompany the card.
  await sleep(2000)
  socket._handleMessage = handle
  const sameDocument = (window as unknown as { __e2eLoadId: string }).__e2eLoadId === loadId
  await record(
    'history-live-update',
    sameDocument &&
      location.pathname === route &&
      frames.some(f => f.startsWith('clipboard:') || f.startsWith('search:')),
    {
      framesAfterArm: frames,
      cardAfterMs,
      cardsAfter: cards().length,
      newestFirst: cards()[0] === card,
      text: card.textContent?.slice(0, 80),
    }
  )
  await sleep(1000)
  await control('exit')
}
// A received single image in the real history UI. Which renderer it takes is a property of the shared frontend:
// a single image uses the daemon's resource bytes (a `blob:` URL). The only `/host-file` consumer is the thumbnail
// grid of an image *group* (one entry with several image files), and a group entry cannot be produced without the
// system clipboard (the daemon sends one entry per file and rejects directories), which isolated runs disable.
// So this scenario pins down the boundary instead of faking a consumer: the single image decodes in the DOM and
// no `/host-file` request was made for it.
// The History page built on `@/ui-kit` (macOS and Linux builds) against the real daemon of an isolated profile.
// The orchestrator (e2e/history_kit_ui_run.py) seeds synthetic entries and passes their texts as the
// secret; `shot:<name>` steps ask it to capture the window, so the driver pauses after each.
async function runHistoryKitScenario() {
  const seeded = JSON.parse(String(await Call.ByName('main.EvidenceService.Secret'))) as {
    code: string
    link: string
    texts: string[]
    image: { dimensions: string }
    zoom: boolean
    systemKeys?: boolean
    shotPauseMs: number
    // Themes of the light/dark pass (default both); a runner can leave one out to get past a host crash.
    themes?: Array<'light' | 'dark'>
  }
  const shot = async (name: string, detail?: unknown) => {
    await record(`shot:${name}`, true, detail)
    await sleep(seeded.shotPauseMs)
  }
  // Shortcut hooks match the physical key (`code`), so letters carry it too.
  const key = (target: EventTarget, name: string, init: KeyboardEventInit = {}) =>
    target.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: name,
        code: /^[a-z]$/i.test(name) ? `Key${name.toUpperCase()}` : name,
        bubbles: true,
        cancelable: true,
        ...init,
      })
    )
  // The app theme changes the way a user changes it: Settings > Appearance, then back to History.
  const setTheme = async (theme: 'light' | 'dark' | 'system') => {
    navControl('/settings')!.click()
    await waitFor('settings page', () => settingsPageOpen())
    const appearance = await waitFor('appearance category', () =>
      Array.from(document.querySelectorAll<HTMLElement>('button')).find(
        b => b.textContent?.trim() === i18n.t('settings.categories.appearance')
      )
    )
    appearance.click()
    const radio = await waitFor('theme choice', () =>
      $(`input[name="appearance-theme"][value="${theme}"]`)
    )
    radio.click()
    await waitFor('theme saved', () => (radio as HTMLInputElement).checked)
    history.back()
    await waitFor(
      'history again',
      () => location.pathname.startsWith('/history') && $('[data-ui-skin]')
    )
  }
  const appliedScheme = () =>
    document.documentElement.classList.contains('dark') ? 'dark' : 'light'
  const materials = () => ({
    hostReported: hostMaterial,
    document: document.documentElement.dataset.windowMaterial ?? null,
    sidebar: $('[data-surface="sidebar"]')?.dataset.windowMaterial ?? null,
    content: $('[data-surface="content"]')?.dataset.windowMaterial ?? null,
    appScheme: appliedScheme(),
    systemScheme: matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
  })
  let hostMaterial = 'unread'
  subscribeWindowMaterial(material => {
    hostMaterial = material
  })
  const rowTitles = () => historyRows().map(row => row.getAttribute('aria-label') ?? '')
  const selected = () => historyRows().filter(row => row.getAttribute('aria-selected') === 'true')
  // Every role attribute in the rendered page must be a WAI-ARIA 1.2 role: a component prop named
  // like an ARIA attribute (Text's type-ramp `role`) must never reach the DOM.
  const ARIA_ROLES = new Set(
    (
      'alert alertdialog application article banner blockquote button caption cell checkbox code ' +
      'columnheader combobox comment complementary contentinfo definition deletion dialog document ' +
      'emphasis feed figure form generic grid gridcell group heading img insertion link list ' +
      'listbox listitem log main mark marquee math menu menubar menuitem menuitemcheckbox ' +
      'menuitemradio meter navigation none note option paragraph presentation progressbar radio ' +
      'radiogroup region row rowgroup rowheader scrollbar search searchbox separator slider ' +
      'spinbutton status strong subscript suggestion superscript switch tab table tablist tabpanel ' +
      'term textbox time timer toolbar tooltip tree treegrid treeitem'
    ).split(' ')
  )
  const ariaRoles = () => {
    const found = Array.from(document.querySelectorAll('[role]'))
    const invalid = found
      .filter(el => (el.getAttribute('role') ?? '').split(/\s+/).some(r => !ARIA_ROLES.has(r)))
      .map(el => `${el.tagName.toLowerCase()}[role=${el.getAttribute('role')}]`)
    return { checked: found.length, invalid }
  }
  const recordAriaRoles = async (state: string) => {
    const audit = ariaRoles()
    await record(`kit-aria-roles-${state}`, audit.invalid.length === 0, audit)
  }

  // Every route change with its caller, so an unexpected navigation names its source.
  const routes: string[] = []
  for (const method of ['pushState', 'replaceState'] as const) {
    const original = history[method].bind(history)
    history[method] = (data: unknown, unused: string, url?: string | URL | null) => {
      routes.push(
        `${method} ${String(url)} ${new Error().stack?.split('\n').slice(2, 6).join(' < ')}`
      )
      original(data, unused, url)
    }
  }
  window.addEventListener('popstate', () => routes.push(`popstate ${location.pathname}`))
  const list = () => {
    const found = $('[role="listbox"][aria-multiselectable]')
    if (!found)
      throw new Error(
        `history list missing at ${location.pathname}: ${routes.slice(-6).join(' | ')}`
      )
    return found
  }
  await reachMainLayout()
  // Window facts for every resize step: what the page sees and what the host reports, so a window manager that
  // does not honour a size request shows up as data instead of a bare timeout.
  const windowFacts = async () => ({
    inner: [innerWidth, innerHeight],
    outer: [outerWidth, outerHeight],
    dpr: devicePixelRatio,
    screen: [screen.width, screen.height, screen.availWidth, screen.availHeight],
    host: await Window.Size().then(
      size => [size.width, size.height],
      error => `Size failed: ${error}`
    ),
  })
  // The history checks below assume a normal (wide) window. A tiling window manager may open it narrower than the
  // configured default; ask for the default size once, record what happened, and carry on either way: the narrow-window
  // checks later are separate rows, and a window that never gets wide fails this row with the observed numbers.
  const normalRequest =
    innerWidth > 905
      ? 'not needed'
      : await Window.SetSize(1100, 720).then(
          () => 'resolved',
          error => `rejected: ${error}`
        )
  for (let waited = 0; waited < 8000 && innerWidth <= 905; waited += 250) await sleep(250)
  await record('kit-window-normal', innerWidth > 905, { normalRequest, ...(await windowFacts()) })
  const root = await waitFor('ui kit page', () => $('[data-ui-skin]'))
  // First launch (design MA1): the profile has no history yet, so the list shows the getting-started
  // steps and the detail explains where items come from. The orchestrator seeds entries after this.
  const steps = await waitFor('getting started', () =>
    $(`ol[aria-label="${i18n.t('history.kit.gettingStarted')}"]`)
  )
  await record(
    'kit-first-launch',
    steps.querySelectorAll('li').length === 3 &&
      !!document.body.textContent?.includes(i18n.t('history.kit.nothingCopied')),
    {
      steps: Array.from(steps.querySelectorAll('li')).map(li => li.textContent?.slice(0, 60)),
    }
  )
  await shot('first-launch')
  await record('kit-ready-for-entries', true)
  // Entries sent from this profile's CLI are not always pushed to the page (see
  // history_live_run.py), so wait for the daemon to hold them, then reload the list the way a user
  // would: Pinned, then All Items.
  const end = Date.now() + 120000
  for (;;) {
    const listed = await daemonClient.request<{ data: Array<{ id: string }> }>(
      '/clipboard/entries?limit=50&offset=0'
    )
    if (listed.data.length >= 5) break
    if (Date.now() > end) throw new Error(`timeout: seeded entries (${listed.data.length})`)
    await sleep(1000)
  }
  const sidebarRow = (label: string) =>
    Array.from(document.querySelectorAll<HTMLElement>('nav button')).find(b =>
      b.textContent?.trim().startsWith(label)
    )
  sidebarRow(i18n.t('history.sidebar.pinned'))?.click()
  await sleep(500)
  sidebarRow(i18n.t('history.sidebar.allItems'))?.click()
  await record('kit-page-mounted', root.dataset.uiSkin === expectedSkin(), {
    skin: root.dataset.uiSkin,
  })
  // Command-F before Settings was ever opened (page shortcut scope on a fresh page).
  await waitFor('list before probe', () => $('[role="listbox"][aria-multiselectable]'))
  key(list(), 'f', modifier())
  await sleep(300)
  await record(
    'kit-search-shortcut-fresh',
    document.activeElement?.getAttribute('type') === 'search',
    {
      active: document.activeElement?.tagName,
    }
  )

  // Following the system appearance, the window material shows behind the sidebar.
  const systemScheme = () => (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
  await setTheme('system')
  await waitFor('system theme applied', () => appliedScheme() === systemScheme())
  const expected = [seeded.code, seeded.link, ...seeded.texts]
  const geometry = () => {
    const box = (selector: string) => {
      const r = $(selector)?.getBoundingClientRect()
      return r ? [Math.round(r.width), Math.round(r.height)] : null
    }
    return {
      root: box('[data-ui-skin]'),
      split: box(skinClass('split')),
      pane: box('[data-pane="list"]'),
      list: box(skinClass('glist')),
      scroller: box(skinClass('glist-scroller')),
      options: document.querySelectorAll('[role="option"]').length,
    }
  }
  await record('kit-geometry', true, geometry())
  await waitFor(
    'seeded rows',
    () => expected.every(text => rowTitles().some(title => title.includes(text.split('\n')[0]))),
    90000
  )
  await record('kit-rows', true, {
    rows: rowTitles().length,
    groups: document.querySelectorAll(`${skinClass('glist-header')}, [role="presentation"]`).length,
  })
  // The host's report decides the marker; a native marker without a host report would be a lie.
  const initial = materials()
  const native = hostMaterial !== 'none'
  // The Linux skin has no native window material: every surface is solid and the host reports none.
  const solidSkin = expectedSkin() === 'linux'
  await record(
    'kit-material',
    initial.content === 'solid' &&
      (solidSkin
        ? initial.sidebar === 'solid' && hostMaterial === 'none'
        : native
          ? initial.sidebar === `native-${hostMaterial}` &&
            initial.document === `native-${hostMaterial}`
          : initial.sidebar === 'css-fallback' && initial.document === null),
    initial
  )
  await shot('system-default', materials())
  await recordAriaRoles('detail')

  // Keyboard: the list is one tab stop; Down moves the selection and the detail follows it.
  list().focus()
  const before = selected()[0]?.dataset.itemId
  key(list(), 'ArrowDown')
  await waitFor('selection moved', () => selected()[0]?.dataset.itemId !== before)
  // Ids, not elements: the virtualized list reuses row elements for other entries.
  const currentId = selected()[0].dataset.itemId!
  await record('kit-keyboard-select', document.activeElement === list(), {
    from: before,
    to: currentId,
    activeDescendant: list().getAttribute('aria-activedescendant'),
  })

  // Return copies the selected entry: the daemon's restore answers 200. The system clipboard is off
  // in E2E, so nothing is read back from it.
  const restores: Array<{ url: string; status: number }> = []
  const fetchBeforeCopy = window.fetch
  window.fetch = async (input, init) => {
    const response = await fetchBeforeCopy(input, init)
    // The path only: the query carries the session token.
    const url = new URL(String(input instanceof Request ? input.url : input), location.href)
      .pathname
    if (url.includes('/clipboard/restore/')) restores.push({ url, status: response.status })
    return response
  }
  key(list(), 'Enter')
  const copied = i18n.t('clipboard.item.actions.copied')
  await waitFor('copy feedback', () =>
    Array.from(document.querySelectorAll('button')).some(b => b.textContent?.includes(copied))
  )
  window.fetch = fetchBeforeCopy
  await record(
    'kit-copy-restored',
    restores.length === 1 &&
      restores[0].status === 200 &&
      restores[0].url.endsWith(`/clipboard/restore/${currentId}`),
    {
      entry: currentId,
      restores,
      systemClipboardReadBack: 'not run (disabled in E2E)',
    }
  )

  // Command-F (page shortcut scope) focuses the search field.
  key(list(), 'f', modifier())
  await sleep(300)
  await record('kit-search-shortcut', document.activeElement?.getAttribute('type') === 'search', {
    active: document.activeElement?.tagName,
  })
  // Command-comma opens Settings from anywhere (global shortcut scope).
  key(list(), ',', { ...modifier(), code: 'Comma' })
  await sleep(500)
  const globalShortcut = location.pathname.startsWith('/settings')
  await record('kit-settings-shortcut', globalShortcut, {
    opened: location.pathname,
  })
  if (globalShortcut) {
    history.back()
    await waitFor(
      'history after probe',
      () => location.pathname.startsWith('/history') && $('[data-ui-skin]')
    )
    await waitFor('rows after probe', () => historyRows().length > 0, 15000).catch(error => {
      const scroller = $(skinClass('glist-scroller'))
      const inner = scroller?.firstElementChild as HTMLElement | null
      throw new Error(
        `${String(error)} ${JSON.stringify({
          ...geometry(),
          scrollerChildren: scroller?.children.length,
          innerHeight: inner?.getBoundingClientRect().height,
          innerHtml: scroller?.innerHTML.slice(0, 400),
        })}`
      )
    })
  }
  list().focus()

  // Pin from the toolbar, then unpin with Command-P: the row's pin mark and the daemon's flag follow.
  const pinnedId = currentId
  const favorited = async () => {
    const response = await daemonClient.request<{
      data: Array<{
        id: string
        is_favorited?: boolean
        isFavorited?: boolean
      }>
    }>('/clipboard/entries?limit=50&offset=0')
    const found = response.data.find(e => e.id === pinnedId)
    return Boolean(found?.is_favorited ?? found?.isFavorited)
  }
  const pinMark = () => !!historyRow(pinnedId)?.querySelector('[data-tone="highlight"]')
  const pinLabel = i18n.t('history.detail.pin')
  const toolbarPin = Array.from(
    document.querySelectorAll<HTMLElement>('[role="toolbar"] button')
  ).find(b => b.getAttribute('aria-label') === pinLabel)
  toolbarPin?.click()
  const pinnedByButton = await waitFor('pin mark', pinMark, 10000).catch(() => false)
  await record('kit-pin-toolbar', Boolean(pinnedByButton) && (await favorited()), {
    entry: pinnedId,
    button: Boolean(toolbarPin),
    mark: pinMark(),
  })
  list().focus()
  key(list(), 'p', modifier())
  const unpinned = await waitFor('pin mark gone', () => !pinMark(), 10000).catch(() => false)
  await record('kit-unpin-shortcut', Boolean(unpinned) && !(await favorited()), {
    entry: pinnedId,
    mark: pinMark(),
    active: document.activeElement?.getAttribute('role'),
  })

  // T opens the tag field of the selected item; a new name is created and attached (design MC1).
  const tagName = `kit${Date.now() % 100000}`
  key(list(), 't', { code: 'KeyT' })
  const tagField = (await waitFor('tag field', () =>
    $(`input[role="combobox"][aria-label="${i18n.t('history.tags.inputLabel')}"]`)
  )) as HTMLInputElement
  const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
  valueSetter.call(tagField, tagName)
  tagField.dispatchEvent(new Event('input', { bubbles: true }))
  await waitFor('create suggestion', () =>
    Array.from(document.querySelectorAll('[role="option"]')).some(o =>
      o.textContent?.includes(tagName)
    )
  )
  await shot('tag-field')
  // Evidence by stage, so a missing chip names where it stopped: which entry the list and the
  // detail show around Enter, and the tag requests with their timing (paths only, no bodies).
  const tagRequests: Array<{
    method: string
    path: string
    status?: number
    ms: number[]
  }> = []
  const fetchBeforeTag = window.fetch
  const tagStart = performance.now()
  window.fetch = async (input, init) => {
    const path = new URL(String(input instanceof Request ? input.url : input), location.href)
      .pathname
    const entry = {
      method: init?.method ?? (input instanceof Request ? input.method : 'GET'),
      path,
      ms: [Math.round(performance.now() - tagStart)],
    } as (typeof tagRequests)[number]
    if (/tag|search\/query/.test(path)) tagRequests.push(entry)
    const response = await fetchBeforeTag(input, init)
    entry.status = response.status
    entry.ms.push(Math.round(performance.now() - tagStart))
    return response
  }
  const stage = () => {
    const detail = $('[data-testid="clipboard-detail"]')
    return {
      listSelected: selected().map(row => row.dataset.itemId),
      detailEntry: detail?.dataset.entryId ?? null,
      detailChips: Array.from(detail?.querySelectorAll(skinClass('tag')) ?? []).map(
        c => c.textContent
      ),
      tagGroup: !!detail?.querySelector('[role="group"]'),
      tagTotal: $('nav')?.textContent?.match(/· (\d+)/)?.[1] ?? null,
    }
  }
  const beforeEnter = stage()
  key(tagField, 'Enter')
  const chipText = `#${tagName}`
  const chipShown = await waitFor(
    'tag chip in the detail',
    () =>
      Array.from(
        document.querySelectorAll(`[data-testid="clipboard-detail"] ${skinClass('tag')}`)
      ).some(c => c.textContent?.includes(chipText)),
    30000
  ).then(
    () => true,
    () => false
  )
  window.fetch = fetchBeforeTag
  await record('kit-tag-created', chipShown, {
    tag: chipText,
    beforeEnter,
    after: stage(),
    requests: tagRequests,
  })
  if (!chipShown) throw new Error('timeout: tag chip in the detail (see kit-tag-created)')

  // Command-click adds a second row: the detail becomes the selection summary.
  const other = historyRows().find(row => row.dataset.itemId !== pinnedId)!
  other.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, ...modifier() }))
  await waitFor(
    'two selected',
    () => selected().length === 2 && $('[data-testid="clipboard-selection"]')
  )
  await record('kit-multi-select', true, {
    selected: selected().map(r => r.dataset.itemId),
  })
  // The new tag is on one of the two: striped "1 of 2"; clicking it adds it to both (design MC2).
  const partialTag = await waitFor(
    'partial tag',
    () =>
      Array.from(
        document.querySelectorAll<HTMLElement>(
          `[data-testid="clipboard-selection"] ${skinClass('tag')}[data-partial]`
        )
      ).find(c => c.textContent?.includes(chipText)),
    30000
  )
  await shot('multi-select')
  await recordAriaRoles('selection')
  partialTag.querySelector<HTMLElement>('button')!.click()
  await waitFor(
    'tag on all',
    () =>
      Array.from(
        document.querySelectorAll<HTMLElement>(
          `[data-testid="clipboard-selection"] ${skinClass('tag')}`
        )
      ).some(c => c.textContent?.includes(chipText) && !c.hasAttribute('data-partial')),
    30000
  )
  await record('kit-tag-added-to-all', true, { tag: chipText })

  // Manage Tags (design MC3): the sheet's table lists the tag; its row menu renames it and the
  // switch puts it in the sidebar.
  const manage = Array.from(document.querySelectorAll<HTMLElement>('nav button')).find(
    b => b.getAttribute('aria-label') === i18n.t('history.tags.manage')
  )!
  manage.click()
  const sheetRow = await waitFor('tag in the manager', () =>
    Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"] [role="row"]')).find(r =>
      r.textContent?.includes(chipText)
    )
  )
  const cell = sheetRow.querySelector<HTMLElement>('[role="gridcell"]')!
  const box = cell.getBoundingClientRect()
  const at = {
    bubbles: true,
    cancelable: true,
    clientX: box.x + 20,
    clientY: box.y + 10,
  }
  cell.dispatchEvent(new MouseEvent('mousedown', { ...at, button: 2 }))
  cell.dispatchEvent(new MouseEvent('contextmenu', { ...at, button: 2 }))
  const renameItem = await waitFor('rename item', () =>
    Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
      i => i.textContent?.trim() === i18n.t('history.tags.rename')
    )
  )
  await record('kit-tag-manager-menu', true, {
    items: Array.from(document.querySelectorAll('[role="menuitem"]')).map(i =>
      i.textContent?.trim()
    ),
  })
  // Over the host's native material a menu must hide what is under it: its own background is
  // nearly opaque (measured in the real WebView while the host reports native glass).
  const menuElement = renameItem.closest<HTMLElement>('[role="menu"]')
  const menuBackground = menuElement ? getComputedStyle(menuElement).backgroundColor : null
  // rgb() is opaque, rgba() carries alpha; anything else (or no menu) fails the step.
  const menuAlpha = (() => {
    const rgba = menuBackground?.match(/^rgba\(([^)]+)\)$/)
    if (rgba) {
      const parts = rgba[1].split(',').map(Number)
      return parts.length === 4 ? parts[3] : null
    }
    return menuBackground && /^rgb\([^)]+\)$/.test(menuBackground) ? 1 : null
  })()
  const menuMaterial = document.documentElement.dataset.windowMaterial ?? null
  await record(
    'kit-native-menu-opaque',
    hostMaterial === 'none' ||
      (menuMaterial?.startsWith('native') === true && menuAlpha !== null && menuAlpha >= 0.9),
    {
      hostReported: hostMaterial,
      document: menuMaterial,
      menuClass: menuElement?.className ?? null,
      background: menuBackground,
      alpha: menuAlpha,
    }
  )
  await shot('tag-manager-menu')
  renameItem.click()
  const renameField = (await waitFor('rename field', () =>
    $(`[role="dialog"] input[aria-invalid], [role="dialog"] ${skinClass('field')}`)
  )) as HTMLInputElement
  const renamed = `${tagName}r`
  valueSetter.call(renameField, renamed)
  renameField.dispatchEvent(new Event('input', { bubbles: true }))
  key(renameField, 'Enter')
  const renamedRow = await waitFor(
    'renamed tag',
    () =>
      Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"] [role="row"]')).find(r =>
        r.textContent?.includes(`#${renamed}`)
      ),
    30000
  )
  renamedRow.querySelector<HTMLElement>('[role="switch"]')!.click()
  await waitFor(
    'tag in the sidebar',
    () =>
      Array.from(document.querySelectorAll('nav button')).some(b =>
        b.textContent?.includes(`#${renamed}`)
      ),
    30000
  )
  await record('kit-tag-manager-rename-sidebar', true, { tag: `#${renamed}` })
  await shot('tag-manager')
  await recordAriaRoles('tag-manager')
  Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"] button'))
    .find(b => b.textContent?.trim() === i18n.t('history.composite.done'))!
    .click()
  await waitFor('manager closed', () => !$('[role="dialog"]'))
  list().focus()

  // Delete asks first; focus starts on Cancel, Escape keeps everything.
  const countBefore = historyRows().length
  key(list(), 'Backspace')
  const alert = await waitFor('delete alert', () => $('[role="alertdialog"]'))
  await waitFor(
    'cancel focused',
    () => document.activeElement?.textContent?.trim() === i18n.t('clipboard.cancelLabel')
  )
  await record('kit-delete-alert', true, {
    focused: document.activeElement?.textContent?.trim(),
  })
  await shot('delete-alert')
  key(document.activeElement ?? alert, 'Escape')
  await waitFor('alert closed', () => !$('[role="alertdialog"]'))
  await sleep(500)
  await record('kit-selection-after-escape', selected().length === 2, {
    selected: selected().map(r => r.dataset.itemId),
  })
  await record('kit-delete-cancelled', historyRows().length === countBefore, {
    rows: historyRows().length,
  })

  // Confirming deletes both selected entries for good.
  const doomed = selected().map(row => row.dataset.itemId!)
  list().focus()
  key(list(), 'Backspace')
  await waitFor('delete alert again', () => $('[role="alertdialog"]'))
  const confirm = Array.from(
    document.querySelectorAll<HTMLElement>('[role="alertdialog"] button')
  ).find(b => b.textContent?.trim() === i18n.t('clipboard.deleteLabel'))!
  confirm.click()
  await waitFor('rows deleted', () => doomed.every(id => !historyRow(id)), 30000).catch(
    async error => {
      const left = await daemonClient.request<{ data: Array<{ id: string }> }>(
        '/clipboard/entries?limit=50&offset=0'
      )
      throw new Error(
        `${String(error)}; doomed ${doomed.join(',')}; daemon has ${left.data.map(e => e.id).join(',')}`
      )
    }
  )
  const remaining = await daemonClient.request<{ data: Array<{ id: string }> }>(
    '/clipboard/entries?limit=50&offset=0'
  )
  await record(
    'kit-delete-confirmed',
    doomed.every(id => !remaining.data.some(e => e.id === id)),
    { deleted: doomed }
  )

  // Hide and show the sidebar with its toolbar control.
  const toggle = () => $(`[aria-expanded][aria-label]:is(${skinClass('split-toggle')})`)
  toggle()!.click()
  await waitFor('sidebar hidden', () => $('[data-sidebar-hidden]'))
  await shot('sidebar-hidden')
  toggle()!.click()
  await waitFor('sidebar shown', () => !$('[data-sidebar-hidden]'))
  await record('kit-sidebar-toggle', true)

  // Grid layout (design ME2): cards in day groups; double click opens the detail as an inspector,
  // Option-Command-I closes it again.
  const viewRadio = (value: 'grid' | 'list') =>
    $(
      `[role="radio"][aria-label="${i18n.t(value === 'grid' ? 'history.kit.viewGrid' : 'history.kit.viewList')}"]`
    )
  viewRadio('grid')!.click()
  const firstCard = await waitFor('grid cards', () => $(`${skinClass('card')}[data-item-id]`))
  await record('kit-grid', !$('[data-testid="clipboard-detail"]'), {
    cards: document.querySelectorAll(`${skinClass('card')}[data-item-id]`).length,
  })
  firstCard.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }))
  firstCard.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
  await waitFor('inspector', () => $('[data-testid="clipboard-detail"]'))
  await shot('grid-inspector')
  key(document.body, 'i', { altKey: true, ...modifier(), code: 'KeyI' })
  await waitFor('inspector closed', () => !$('[data-testid="clipboard-detail"]'))
  await record('kit-grid-inspector', true)
  // "Tag…" from the context menu of a card outside the selection, with the inspector closed, acts
  // on that card: it becomes the selection and the inspector opens on its tag field.
  const cards = () =>
    Array.from(document.querySelectorAll<HTMLElement>(`${skinClass('card')}[data-item-id]`))
  const otherCard = cards().find(card => card.dataset.itemId !== firstCard.dataset.itemId)!
  const otherId = otherCard.dataset.itemId!
  const cardBox = otherCard.getBoundingClientRect()
  const cardPoint = {
    bubbles: true,
    cancelable: true,
    clientX: cardBox.x + 20,
    clientY: cardBox.y + 20,
    button: 2,
  }
  otherCard.dispatchEvent(new MouseEvent('contextmenu', cardPoint))
  const tagItem = await waitFor('Tag… item', () =>
    Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item =>
      item.textContent?.trim().startsWith(i18n.t('history.tags.tagEllipsis'))
    )
  )
  tagItem.click()
  await waitFor(
    'tag field for the right-clicked card',
    () =>
      $(`[data-testid="clipboard-detail"][data-entry-id="${otherId}"]`) &&
      $(`input[role="combobox"][aria-label="${i18n.t('history.tags.inputLabel')}"]`)
  )
  await record('kit-grid-context-tag', true, { entry: otherId })
  key(document.activeElement ?? document.body, 'Escape')
  key(document.body, 'i', { altKey: true, ...modifier(), code: 'KeyI' })
  await waitFor('inspector closed again', () => !$('[data-testid="clipboard-detail"]'))
  viewRadio('list')!.click()
  await waitFor(
    'list again',
    () => $('[role="option"][data-item-id]') && $('[data-testid="clipboard-detail"]')
  )

  // Narrowest window (the host's minimum size), then the same at 150% interface scale: nothing
  // spills out of the page, the toolbar and the action bar keep their commands on screen, the list
  // keeps keyboard focus, and the keyboard reaches the last row with the selection in view.
  const layoutCheck = () => {
    // What is on screen: the layout viewport, cut to the visual one (a magnified WKWebView keeps
    // its layout width while showing only part of it).
    const visible = {
      width: Math.min(document.documentElement.clientWidth, visualViewport?.width ?? innerWidth),
      height: Math.min(
        document.documentElement.clientHeight,
        visualViewport?.height ?? innerHeight
      ),
    }
    const viewport = {
      layout: [document.documentElement.clientWidth, document.documentElement.clientHeight],
      visual: [
        Math.round(visualViewport?.width ?? innerWidth),
        Math.round(visualViewport?.height ?? innerHeight),
      ],
    }
    const outside = (
      selector: string,
      bounds: DOMRect | { left: number; right: number; top: number; bottom: number }
    ) =>
      Array.from(document.querySelectorAll<HTMLElement>(selector))
        .filter(el => el.offsetParent !== null)
        .filter(el => {
          const r = el.getBoundingClientRect()
          return (
            r.left < bounds.left - 0.5 ||
            r.right > bounds.right + 0.5 ||
            r.top < bounds.top - 0.5 ||
            r.bottom > bounds.bottom + 0.5
          )
        })
        .map(
          el => el.getAttribute('aria-label') ?? el.textContent?.trim().slice(0, 24) ?? el.className
        )
    const screen = {
      left: 0,
      right: visible.width,
      top: 0,
      bottom: visible.height,
    }
    const bar = $(skinClass('pane-actionbar'))?.getBoundingClientRect()
    return {
      viewport,
      scale: Math.round(devicePixelRatio * 100) / 100,
      pageOverflowX: document.documentElement.scrollWidth - document.documentElement.clientWidth,
      toolbarOffscreen: outside(
        `${skinClass('split-toolbar')} button, ${skinClass('split-toolbar')} [role="radio"], ${skinClass('split-toolbar')} input`,
        screen
      ),
      actionBarOffscreen: bar
        ? [
            ...outside(
              `${skinClass('pane-actionbar')} > *:not(${skinClass('actionbar-hint')})`,
              bar
            ),
            ...outside(skinClass('pane-actionbar'), screen),
          ]
        : ['no action bar'],
      panesOffscreen: outside('[data-pane]', screen),
      panes: Array.from(document.querySelectorAll<HTMLElement>('[data-pane]')).map(pane => [
        pane.dataset.pane,
        Math.round(pane.getBoundingClientRect().width),
      ]),
    }
  }
  const layoutOk = (state: ReturnType<typeof layoutCheck>) =>
    state.pageOverflowX <= 0 &&
    state.toolbarOffscreen.length === 0 &&
    state.actionBarOffscreen.length === 0 &&
    state.panesOffscreen.length === 0 &&
    state.panes.every(([, width]) => Number(width) > 0)
  const reachLastRow = async () => {
    list().focus()
    key(list(), 'End')
    const last = await waitFor('last row selected', () => {
      const rows = historyRows()
      const row = selected()[0]
      return row && rows.length > 0 && row === rows[rows.length - 1] ? row : null
    })
    const scroller =
      list().closest(skinClass('glist'))?.getBoundingClientRect() ?? list().getBoundingClientRect()
    const r = last.getBoundingClientRect()
    return {
      focused: document.activeElement === list(),
      inView: r.top >= scroller.top - 0.5 && r.bottom <= scroller.bottom + 0.5,
    }
  }
  const originalSize = await Window.Size()
  await record('kit-window-original', true, {
    original: [originalSize.width, originalSize.height],
    ...(await windowFacts()),
  })
  const narrowRequest = await Window.SetSize(900, 600).then(
    () => 'resolved',
    error => `rejected: ${error}`
  )
  await waitFor('narrow window', () => innerWidth <= 905)
  await record('kit-window-narrow-facts', true, {
    request: [900, 600],
    narrowRequest,
    ...(await windowFacts()),
  })
  await sleep(600)
  const narrow = layoutCheck()
  const narrowKeys = await reachLastRow()
  await record('kit-narrow-window', layoutOk(narrow) && narrowKeys.focused && narrowKeys.inView, {
    ...narrow,
    ...narrowKeys,
  })
  await shot('narrow')
  // The compact window hides the sidebar; its toolbar toggle still reaches it.
  // Queried each time: the toggle moves into the sidebar's header when it opens.
  const sidebarToggle = () => $(`[aria-expanded][aria-label]:is(${skinClass('split-toggle')})`)!
  sidebarToggle().click()
  const drawer = await waitFor('sidebar in the compact window', () => {
    const nav = $('[data-surface="sidebar"]')
    const r = nav?.getBoundingClientRect()
    return r && r.width > 100 && r.left >= -0.5 && r.right <= innerWidth + 0.5 ? r : null
  })
  const drawerLayout = layoutCheck()
  await record('kit-narrow-sidebar', drawerLayout.pageOverflowX <= 0, {
    sidebarWidth: Math.round(drawer.width),
    ...drawerLayout,
  })
  await shot('narrow-sidebar')
  sidebarToggle().click()
  await waitFor('sidebar hidden again', () => $('[data-sidebar-hidden]'))
  // Interface scale (Settings > Appearance) at the narrowest window: off by default, because the
  // macOS host magnifies the WKWebView without relaying out (see history_kit_ui_run.py --zoom).
  if (seeded.zoom) {
    setUiScale(1.5)
    await waitFor('150% scale', () => (visualViewport?.width ?? innerWidth) <= 905 / 1.4)
    await sleep(600)
    const zoomed = layoutCheck()
    const zoomedKeys = await reachLastRow()
    await record('kit-zoom-150', layoutOk(zoomed) && zoomedKeys.focused && zoomedKeys.inView, {
      ...zoomed,
      ...zoomedKeys,
    })
    await shot('narrow-zoom-150')
    setUiScale(1)
  }
  const restored = () => Math.abs(innerWidth - originalSize.width) <= 2
  const restoreRequest = await Window.SetSize(originalSize.width, originalSize.height).then(
    () => 'resolved',
    error => `rejected: ${error}`
  )
  // Sample for up to 12 s so the record shows whether the size ever moves; the check below still fails on a timeout.
  const restoreSamples: unknown[] = []
  for (let elapsed = 0; elapsed <= 12000 && !restored(); elapsed += 1000) {
    restoreSamples.push({ ms: elapsed, ...(await windowFacts()) })
    await sleep(1000)
  }
  await record('kit-window-restore-facts', restored(), {
    request: [originalSize.width, originalSize.height],
    restoreRequest,
    samples: restoreSamples,
    final: await windowFacts(),
  })
  await waitFor('window restored', restored)
  list().focus()
  key(list(), 'Home')
  await sleep(300)

  // A real key press: the runner types Ctrl+B through the kernel's uinput device (ydotool), so the compositor delivers it to the
  // focused window like a keyboard would. Only keydown events with isTrusted === true count; a dispatched DOM event never does.
  if (seeded.systemKeys) {
    const hidden = () => !!$('[data-sidebar-hidden]')
    const seen: unknown[] = []
    const onKeyDown = (event: KeyboardEvent) =>
      seen.push({ key: event.key, ctrl: event.ctrlKey, trusted: event.isTrusted })
    addEventListener('keydown', onKeyDown, true)
    const toggle = $(`[aria-expanded][aria-label]:is(${skinClass('split-toggle')})`)
    const pressed = async (step: string, expectHidden: boolean) => {
      await record(`kit-ready-for-${step}`, true, {
        sidebarHidden: hidden(),
        hasFocus: document.hasFocus(),
      })
      for (let waited = 0; waited < 10000 && hidden() !== expectHidden; waited += 100)
        await sleep(100)
      const realCtrlB = seen.some(
        e =>
          (e as { key: string; ctrl: boolean; trusted: boolean }).key.toLowerCase() === 'b' &&
          (e as { ctrl: boolean }).ctrl &&
          (e as { trusted: boolean }).trusted
      )
      await record(`kit-${step}`, hidden() === expectHidden && realCtrlB, {
        sidebarHidden: hidden(),
        expectedHidden: expectHidden,
        keydowns: seen,
        toggleHint: toggle
          ? {
              label: toggle.getAttribute('aria-label'),
              title: toggle.getAttribute('title'),
              keys: toggle.getAttribute('aria-keyshortcuts'),
            }
          : null,
      })
      seen.length = 0
    }
    const startHidden = hidden()
    await pressed('system-ctrl-b-first', !startHidden)
    await pressed('system-ctrl-b-second', startHidden)
    removeEventListener('keydown', onKeyDown, true)
    list().focus()
  }

  // Search (design MB1-MB3): text with match highlighting, a device token from the suggestions,
  // then a search with no results and its widening choices.
  const field = () => $('input[role="combobox"]') as HTMLInputElement
  const typeInto = (text: string) => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
    setter.call(field(), text)
    field().dispatchEvent(new Event('input', { bubbles: true }))
  }
  // The oldest seeded text is never selected above, so it survives the deletes.
  const word = seeded.texts[0].split(/\W+/).sort((a, b) => b.length - a.length)[0]
  field().focus()
  typeInto(word)
  key(field(), 'Enter')
  await waitFor(
    'text results',
    () => historyRows().length === 1 && $('[role="option"] mark')?.textContent === word,
    30000
  )
  await record('kit-search-text', true, {
    rows: rowTitles(),
    mark: $('[role="option"] mark')?.textContent,
  })
  await shot('search-text')
  // `#` lists the tags (the isolated profile has no paired device for `@`).
  const suggestionOptions = () =>
    Array.from(
      document.querySelectorAll<HTMLElement>(
        '[role="listbox"]:not([aria-multiselectable]) [role="option"]'
      )
    )
  typeInto('#')
  await waitFor('tag suggestions', () => suggestionOptions().length > 0)
  await record('kit-search-suggestions', field().getAttribute('aria-expanded') === 'true', {
    options: suggestionOptions().map(o => o.textContent?.slice(0, 40)),
    activeDescendant: field().getAttribute('aria-activedescendant'),
  })
  await shot('search-suggestions')
  key(field(), 'ArrowDown')
  key(field(), 'Enter')
  await waitFor('tag token', () => $(`${skinClass('token-search')} [data-kind="tag"]`))
  await record('kit-search-token', true, {
    token: $(`${skinClass('token-search')} [data-kind="tag"]`)?.textContent,
  })
  // No remaining entry carries the link tag: the empty state offers to drop the condition.
  const empty = await waitFor('no results', () => $('[role="status"]'), 30000)
  await waitFor('widening choices', () => empty.querySelectorAll('button').length > 0, 30000)
  await record('kit-search-no-results', true, {
    text: empty.textContent?.slice(0, 160),
    choices: Array.from(empty.querySelectorAll('button')).map(b => b.textContent),
  })
  await shot('search-no-results')
  const clear = Array.from(document.querySelectorAll<HTMLElement>('button')).find(
    b => b.getAttribute('aria-label') === i18n.t('history.composite.clearAll')
  )!
  clear.click()
  await waitFor('search cleared', () => historyRows().length >= 3, 30000)
  await record(
    'kit-search-cleared',
    !($('input[role="combobox"]') as HTMLInputElement | null)?.value,
    {
      rows: historyRows().length,
    }
  )

  // A failing list query is an error with Retry, not an empty history (fault injected in the page).
  const realFetch = window.fetch
  window.fetch = (input, init) =>
    String(input instanceof Request ? input.url : input).includes('/search/query')
      ? Promise.resolve(new Response('{"error":"injected"}', { status: 500 }))
      : realFetch(input, init)
  field().focus()
  typeInto(word)
  key(field(), 'Enter')
  const failed = await waitFor(
    'query failure state',
    () =>
      Array.from(document.querySelectorAll<HTMLElement>('[role="status"]')).find(el =>
        el.textContent?.includes(i18n.t('history.kit.queryFailedTitle'))
      ),
    30000
  )
  // No count either: a failed query does not know how many items match.
  const failureCount = {
    subtitle: $(skinClass('split-subtitle'))?.textContent ?? null,
    zeroResults:
      $('[data-pane="list"]')?.textContent?.includes(
        i18n.t('history.composite.results', { count: 0 })
      ) ?? false,
  }
  await record(
    'kit-query-failure',
    historyRows().length === 0 && failureCount.subtitle === null && !failureCount.zeroResults,
    { text: failed.textContent?.slice(0, 120), ...failureCount }
  )
  await shot('query-failure')
  window.fetch = realFetch
  Array.from(failed.querySelectorAll('button'))
    .find(b => b.textContent === i18n.t('history.kit.retry'))!
    .click()
  await waitFor('rows after retry', () => historyRows().length === 1, 30000)
  await record('kit-query-retry', true, { rows: rowTitles() })
  Array.from(document.querySelectorAll<HTMLElement>('button'))
    .find(b => b.getAttribute('aria-label') === i18n.t('history.composite.clearAll'))!
    .click()
  await waitFor('search cleared again', () => historyRows().length >= 3, 30000)

  // An image entry (design ME1/ME4): the orchestrator uploads a synthetic PNG through a loopback
  // mobile device of this profile. The detail shows it with Fit / 100%, its facts, and Quick Look.
  await record('kit-ready-for-image', true)
  const imageRow = async () => {
    const thumb = $('[role="option"][data-item-id] [data-kind="image"]')
    return thumb?.closest<HTMLElement>('[role="option"]') ?? null
  }
  const imageEnd = Date.now() + 120000
  for (;;) {
    if (await imageRow()) break
    if (Date.now() > imageEnd) throw new Error('timeout: image entry')
    // Uploads are not always pushed to the page either: reload the list as above.
    sidebarRow(i18n.t('history.sidebar.pinned'))?.click()
    await sleep(500)
    sidebarRow(i18n.t('history.sidebar.allItems'))?.click()
    await sleep(1500)
  }
  openRow((await imageRow())!)
  const preview = await waitFor(
    'image preview',
    () => $(`[data-testid="clipboard-detail"] ${skinClass('preview')}[data-kind="image"] img`),
    60000
  )
  const facts = () =>
    $('[data-testid="clipboard-detail"]')?.textContent?.includes(seeded.image.dimensions) ?? false
  await waitFor('image facts', facts, 30000)
  const zoom = (label: string) =>
    Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid="clipboard-detail"] [role="radio"]')
    ).find(b => b.textContent === label)
  await record('kit-image-detail', true, {
    naturalWidth: (preview as HTMLImageElement).naturalWidth,
    fit: zoom(i18n.t('history.detail.imageFit'))?.getAttribute('aria-checked'),
  })
  await shot('image-detail')
  zoom(i18n.t('history.detail.imageActual'))!.click()
  await waitFor(
    '100% zoom',
    () => zoom(i18n.t('history.detail.imageActual'))?.getAttribute('aria-checked') === 'true'
  )
  await record('kit-image-actual-size', true)
  zoom(i18n.t('history.detail.imageFit'))!.click()
  Array.from(document.querySelectorAll<HTMLElement>('[data-testid="clipboard-detail"] button'))
    .find(b => b.textContent?.startsWith(i18n.t('history.detail.quickLook')))!
    .click()
  const quickLook = await waitFor('quick look', () => $(`${skinClass('quicklook')} img`), 30000)
  await waitFor('quick look decoded', () => (quickLook as HTMLImageElement).naturalWidth > 0)
  await record('kit-quick-look', true, {
    title: $(skinClass('quicklook-title'))?.textContent,
  })
  await shot('quick-look')
  key(document.activeElement ?? document.body, 'Escape')
  await waitFor('quick look closed', () => !$(skinClass('quicklook')))
  await record('kit-quick-look-closed', true)

  // Light and dark themes: the material stays native only while the theme matches the system
  // appearance; otherwise the sidebar shows its CSS fallback (the glass would be drawn in the wrong
  // appearance, Wails sets NSAppearance only at window creation).
  for (const theme of seeded.themes ?? (['light', 'dark'] as const)) {
    await setTheme(theme)
    await waitFor(`${theme} theme applied`, () => appliedScheme() === theme)
    // Settings write-back: the choice made in the page is what the daemon holds for the profile.
    let stored = false
    for (const end = Date.now() + 10000; !stored && Date.now() < end; await sleep(300)) {
      stored = (await getSettings()).general.theme === theme
    }
    await record(`kit-settings-writeback-${theme}`, stored, { theme })
    await sleep(500)
    const state = materials()
    // Without a host material (no private_mac_apis, or an opaque webview) the sidebar always
    // falls back and the page stays opaque.
    const expected = solidSkin
      ? 'solid'
      : hostMaterial !== 'none' && theme === systemScheme()
        ? `native-${hostMaterial}`
        : 'css-fallback'
    await record(
      `kit-material-${theme}`,
      state.sidebar === expected &&
        state.content === 'solid' &&
        (expected === 'css-fallback' || expected === 'solid'
          ? state.document === null
          : state.document === expected),
      { ...state, expected }
    )
    await shot(`${theme}-default`, state)
  }
  await setTheme('system')

  // Paging past the daemon's 200-result clamp: the orchestrator adds a few hundred entries, and
  // End, pressed as the list loads more, must reach the oldest entry instead of stopping at 200.
  await record('kit-ready-for-bulk', true)
  // The runner seeds 230 entries one request at a time: measured 227 s (Xvfb) to 252 s (niri) here, so the budget has to exceed that.
  const bulkBudget = 480000
  const bulkEnd = Date.now() + bulkBudget
  // The daemon is busy ingesting while the runner seeds, so one poll may exceed the client's request timeout. Retry until
  // the deadline, but keep every failure in the record: a daemon that never answers still ends in the timeout below.
  const pollFailures: string[] = []
  for (;;) {
    const page = await daemonClient
      .request<{ data: Array<{ id: string }> }>('/clipboard/entries?limit=1&offset=210')
      .catch((error: unknown) => {
        pollFailures.push(`${Math.round((Date.now() - (bulkEnd - bulkBudget)) / 1000)}s: ${error}`)
        return null
      })
    if (page && page.data.length > 0) break
    if (Date.now() > bulkEnd)
      throw new Error(`timeout: bulk entries (poll failures: ${pollFailures.join('; ')})`)
    await sleep(1000)
  }
  await record('kit-bulk-poll', true, { transientFailures: pollFailures })
  sidebarRow(i18n.t('history.sidebar.pinned'))?.click()
  await sleep(500)
  sidebarRow(i18n.t('history.sidebar.allItems'))?.click()
  const oldest = seeded.texts[0]
  const reachedOldest = () => selected()[0]?.getAttribute('aria-label')?.includes(oldest) ?? false
  const listbox = () => $('[role="listbox"][aria-multiselectable]')
  await waitFor('bulk list', () => listbox() && historyRows().length > 0, 60000)
  const scrollEnd = Date.now() + 120000
  let presses = 0
  while (!reachedOldest() && Date.now() < scrollEnd) {
    // While a larger window loads the list is briefly replaced; press End once it is back.
    const current = listbox()
    if (current) {
      current.focus()
      key(current, 'End')
      presses++
    }
    await sleep(800)
  }
  await record('kit-paging-past-200', reachedOldest(), {
    presses,
    selected: selected()[0]?.getAttribute('aria-label')?.slice(0, 60),
    subtitle: $(skinClass('split-subtitle'))?.textContent ?? null,
  })

  await record('history-kit-complete', true)
  await sleep(1500)
  await control('exit')
}

async function runSingleImageUiScenario() {
  const name = String(await Call.ByName('main.EvidenceService.Secret'))
  await reachMainLayout()
  type Entry = { id: string; preview: string }
  const end = Date.now() + 120000
  let entry: Entry | undefined
  while (!entry && Date.now() < end) {
    const response = await daemonClient.request<{ data: Entry[] }>(
      '/clipboard/entries?limit=50&offset=0'
    )
    entry = response.data.find(e => e.preview.includes(name) && /file:\/\//i.test(e.preview))
    if (!entry) await sleep(1000)
  }
  if (!entry) throw new Error(`timeout: entry for ${name}`)
  const card = await waitFor('history card', () => historyRow(entry!.id))
  openRow(card)
  const panel = await waitFor('detail panel', () => $('[data-testid="clipboard-detail"]'))
  const hero = await waitFor(
    'single image in the detail panel',
    () => {
      const img = Array.from(panel.querySelectorAll<HTMLImageElement>('img')).find(
        i => i.alt === name
      )
      return img && img.complete && img.naturalWidth > 0 ? img : null
    },
    60000
  )
  const src = hero.getAttribute('src') ?? ''
  const hostFileRequests = performance
    .getEntriesByType('resource')
    .filter(
      r => r.name.includes('/host-file?path=') && r.name.includes(encodeURIComponent(name))
    ).length
  await record(
    'single-image-uses-daemon-bytes',
    src.startsWith('blob:') && hostFileRequests === 0,
    {
      srcScheme: src.slice(0, 5),
      width: hero.naturalWidth,
      hostFileRequests,
    }
  )
  await sleep(1000)
  await control('exit')
}

// Update scenarios drive the release feed served by the orchestrator: the main window
// starts the flow and the updater window (e2e-secondary.ts) performs the clicks.
async function runUpdateScenario(phase: string) {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  await control('update-state')
  if (phase === 'update-good' && (await installedBundle())) {
    await record('update-relaunched', true)
    await sleep(1500)
    await control('exit')
    return
  }
  await control('update-check')
  // The updater window reports the rest; keep this window alive meanwhile.
  await sleep(120000)
}

// The scheduler scenario never asks for a check: the background scheduler has to open
// the updater window on its own, and must not open it again for the same version.
async function runSchedulerScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  await control('scheduler-wait-updater')
  await sleep(4000) // the orchestrator screenshots the window meanwhile
  await control('close-updater')
  await control('scheduler-quiet')
  await control('exit')
}

// Quick-panel preferences go through the shared frontend settings API (the code path of the
// settings UI); the native side then opens the panel and checks where it landed.
async function runQuickPanelSettingsScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const show = async (label: string) => {
    await control(`panel-show:${label}`)
    await sleep(1500) // the orchestrator captures the screen meanwhile
    await control('panel-hide')
  }
  await control('panel-warp:center')
  await setQuickPanelPosition('center')
  await show('center')
  await control('panel-warp:near')
  await setQuickPanelPosition('follow_cursor')
  await show('follow-near')
  await control('panel-warp:corner')
  await show('follow-flipped')
  await setQuickPanelEnabled(false)
  await show('disabled')
  await setQuickPanelEnabled(true)
  await show('reenabled')
  // The generated binding reports command failures as a result value rather than throwing.
  const refused = await settle(
    HostService.SetQuickPanelDoubleTapModifier(QuickPanelDoubleTapModifier.DoubleTapModifierAlt)
  )
  await record(
    'double-tap-unavailable-rejected',
    refused.status === 'error' && (refused.error as { code?: string }).code === 'Conflict',
    {
      result: refused,
    }
  )
  const accepted = await settle(
    HostService.SetQuickPanelDoubleTapModifier(
      QuickPanelDoubleTapModifier.DoubleTapModifierDisabled
    )
  )
  await record('double-tap-disabled-accepted', accepted.status === 'ok', {
    result: accepted,
  })
  await control('exit')
}

// Native helper supervision: each `act-*` step marks a moment the orchestrator compares the helper
// process table against. The settings go through the same frontend bindings as the settings UI.
async function runNativePanelScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  await record('native-ready', true)
  await sleep(5000) // the helper starts
  await record('act-disable', true)
  await setQuickPanelEnabled(false)
  await sleep(4000)
  await record('act-enable', true)
  await setQuickPanelEnabled(true)
  await sleep(4000)
  await record('act-shortcut', true)
  const shortcut = await settle(
    HostService.UpdateKeyboardShortcuts({
      'global.toggleQuickPanel': 'Ctrl+Alt+Space',
    })
  )
  await record('shortcut-saved', shortcut.status === 'ok', {
    result: shortcut,
  })
  await sleep(4000)
  await record('act-double-tap', true)
  const tap = await settle(
    HostService.SetQuickPanelDoubleTapModifier(QuickPanelDoubleTapModifier.DoubleTapModifierAlt)
  )
  await record('double-tap-saved', tap.status === 'ok', { result: tap })
  const availability = await settle(HostService.GetQuickPanelDoubleTapAvailability())
  await record('double-tap-availability', availability.status === 'ok', {
    result: availability,
  })
  await sleep(4000)
  await record('act-kill', true) // the orchestrator kills the helper now; it must come back
  await sleep(9000)
  await record('act-exit', true)
  await control('exit')
}

// Linux quick panel shortcut through the REAL shared settings page: open Settings > Quick panel, report what the page
// shows (the switch and the effective shortcut, i.e. the frontend default while nothing is stored), then change the
// shortcut with the real recorder popover. The recorder needs real key events: the orchestrator sends them (XTEST)
// after the `recorder-open` step and this scenario saves once the recorder shows a candidate. Nothing here calls a
// host command or the daemon directly. It never turns the quick panel on by itself: a default-off setting stays off
// unless the phase is `linux-shortcut-ui:enable`; `:rebind` runs the recorder flow on an already enabled panel.
async function runLinuxShortcutUiScenario(phase: string) {
  await reachMainLayout()
  const editLabel = `${i18n.t('settings.sections.shortcuts.edit')} ${i18n.t('settings.sections.shortcuts.actions.toggleQuickPanel')}`
  const editButton = () => $(`button[aria-label="${editLabel}"]`)
  const quickPanelSwitch = () => document.querySelector<HTMLElement>('button[role="switch"]')
  const readState = () => ({
    enabled: quickPanelSwitch()?.getAttribute('aria-checked'),
    shortcutLabel: editButton()?.textContent,
  })
  await navigate(
    () => navControl('/settings')?.click(),
    '/settings',
    () => document.querySelectorAll('button[aria-current]').length,
    'settings-open',
    true
  )
  const categoryName = i18n.t('settings.categories.quickPanel')
  await waitFor('quick panel category', () =>
    Array.from(document.querySelectorAll<HTMLElement>('button')).find(
      b => b.textContent?.trim() === categoryName
    )
  ).then(b => b.click())
  await waitFor('quick panel shortcut row', () => editButton())
  await record('ui-quick-panel-state', true, {
    ...readState(),
    locale: i18n.language,
  })
  const action = phase.split(':')[1] ?? 'observe'
  if (action === 'enable') {
    // The product default may be off. Only this explicit phase plays the user turning the switch on (a separate,
    // labelled user action); observe/rebind never change it.
    if (quickPanelSwitch()?.getAttribute('aria-checked') !== 'true') {
      quickPanelSwitch()!.click()
      await waitFor('switch on', () => quickPanelSwitch()?.getAttribute('aria-checked') === 'true')
    }
    await record('ui-enabled', true, readState())
    return
  }
  if (action !== 'rebind' || quickPanelSwitch()?.getAttribute('aria-checked') !== 'true') return
  editButton()!.click()
  const recorder = await waitFor('recorder', () =>
    document.querySelector<HTMLElement>(
      `[role="group"][aria-label="${i18n.t('settings.sections.shortcuts.recording')}"]`
    )
  )
  recorder.focus()
  await record('recorder-open', document.activeElement === recorder)
  const candidate = await waitFor(
    'recorded candidate',
    () => recorder.querySelector('kbd')?.textContent,
    30000
  )
  await record('recorder-candidate', true, { candidate })
  const saveName = i18n.t('settings.sections.shortcuts.save')
  Array.from(document.querySelectorAll<HTMLButtonElement>('button'))
    .find(b => b.textContent?.trim() === saveName)!
    .click()
  await waitFor('saved label', () => editButton()?.textContent === candidate, 15000)
  await record('ui-shortcut-saved', true, readState())
}

// Requests printed by a stand-in helper: show the hidden main window, then open the settings page.
async function runNativeRequestsScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  await control('close-main')
  await control('wait-main-visible')
  await waitFor(
    'settings after open_settings request',
    () => location.pathname.startsWith('/settings'),
    30000
  )
  await record('helper-open-settings', true, { path: location.pathname })
  await sleep(1000)
  await control('exit')
}

// File and directory commands through the shared frontend bindings. Native dialogs are answered by
// the e2e build from the environment, and the system opener records instead of launching.
async function runFileOpsScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const result = async (
    step: string,
    call: Promise<{ status: string }>,
    accept: (r: any) => boolean = r => r.status === 'ok'
  ) => {
    const r = await call
    await record(step, accept(r), { result: r })
  }
  const pngBytes = [137, 80, 78, 71, 13, 10, 26, 10, 0, 1, 2, 3]
  // Go `[]byte` crosses the bridge as base64.
  const png = btoa(String.fromCharCode(...pngBytes))
  const pngHead = btoa(String.fromCharCode(...pngBytes.slice(0, 4)))
  const picked = await settle(HostService.PickDirectory())
  await record('pick-directory-chosen', picked.status === 'ok', {
    result: picked,
  })
  await result('pick-directory-cancelled', settle(HostService.PickDirectory()))
  await result('save-image-cancelled', settle(HostService.SaveImageAs('a.png', png)))
  await result('save-image-saved', settle(HostService.SaveImageAs('../../x/shot.png', png)))
  await result(
    'open-image-first',
    settle(HostService.OpenImageExternally('../../etc/first.png', png))
  )
  await result('open-image-second', settle(HostService.OpenImageExternally('second.png', pngHead)))
  await result('open-data-directory', settle(HostService.OpenDataDirectory()))
  await result('open-logs-directory', settle(HostService.OpenLogsDirectory()))
  const existing = picked.status === 'ok' && picked.data ? picked.data : ''
  await result('reveal-existing', settle(HostService.RevealPath(existing)))
  await result(
    'reveal-missing',
    settle(HostService.RevealPath('/definitely/not/here')),
    r => r.status === 'error' && r.error.code === 'NotFound'
  )
  await result('export-logs-cancelled', settle(HostService.ExportStartupLogs()))
  await result('export-logs-saved', settle(HostService.ExportStartupLogs()))
  await control('exit')
}

// Config package round trip: export, preview (wrong and right password), cancel paths, then stage an
// import over a changed setting. The next launch (config-applied) shows whether the daemon applied it.
async function runConfigExportScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const step = async (
    name: string,
    call: Promise<{ status: string }>,
    accept: (r: any) => boolean
  ) => {
    const r = await call
    await record(name, accept(r), { result: r })
    return r as any
  }
  const passphrase = String(await Call.ByName('main.EvidenceService.Secret'))
  await step(
    'set-position',
    setQuickPanelPosition('follow_cursor').then(() => ({ status: 'ok' })),
    r => r.status === 'ok'
  )
  const exported = await step(
    'export',
    settle(HostService.ExportConfigPackage()),
    r => r.status === 'ok'
  )
  const bundle = exported.data?.path as string
  await step(
    'export-cancelled',
    settle(HostService.ExportConfigPackage()),
    r => r.status === 'error' && r.error.kind === 'cancelled'
  )
  const picked = await step(
    'pick-bundle',
    settle(HostService.PickConfigBundlePath()),
    r => r.status === 'ok' && r.data === bundle
  )
  await step(
    'pick-bundle-cancelled',
    settle(HostService.PickConfigBundlePath()),
    r => r.status === 'ok' && r.data === null
  )
  await step(
    'preview-wrong-password',
    settle(HostService.PreviewConfigImport('definitely-wrong', picked.data)),
    r => r.status === 'error' && r.error.kind === 'daemon'
  )
  await step(
    'preview',
    settle(HostService.PreviewConfigImport(passphrase, bundle)),
    r => r.status === 'ok' && !!r.data.profileId && !!r.data.appVersion
  )
  await setQuickPanelPosition('center')
  await step(
    'import-staged',
    settle(HostService.ImportConfigPackage(passphrase, bundle)),
    r => r.status === 'ok' && r.data.stagedOk === true
  )
  await control('exit')
}

// Launch-at-login: the preference and the OS login item must move together, and a failed OS change must
// roll the preference back. Each phase is one launch; the orchestrator prepares the disk in between.
async function runAutostartScenario(phase: string) {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const set = async (step: string, enabled: boolean, accept: (r: any) => boolean) => {
    const r = await settle(HostService.UpdateAutostart(enabled))
    await record(step, accept(r), { result: r })
  }
  // Records the outcome either way: the orchestrator decides what the platform mechanism is allowed to say.
  const attempt = async (step: string, enabled: boolean) => {
    const r = await settle(HostService.UpdateAutostart(enabled))
    await record(step, true, { result: r })
  }
  if (phase === 'autostart' || phase === 'autostart-bundle') {
    // Repeated enable and repeated disable must both be idempotent; each state is read back from Wails itself.
    await set('enable', true, r => phase === 'autostart-bundle' || r.status === 'ok')
    await control('autostart-state:enabled')
    await set('enable-repeat', true, r => phase === 'autostart-bundle' || r.status === 'ok')
    await control('autostart-state:enabled-repeat')
    await set('disable', false, r => r.status === 'ok')
    await control('autostart-state:disabled')
    await set('disable-repeat', false, r => r.status === 'ok')
    await control('autostart-state:disabled-repeat')
    await set('enable-final', true, r => phase === 'autostart-bundle' || r.status === 'ok')
    await control('autostart-state:enabled-final')
  } else if (phase === 'autostart-cleanup') {
    await attempt('cleanup-disable', false)
    await control('autostart-state:cleaned')
  } else if (phase === 'autostart-refused') {
    await set(
      'enable-refused',
      true,
      r =>
        r.status === 'error' && String(r.error.message).includes('must not change the login item')
    )
    await control('autostart-state:after-refusal')
  } else if (phase === 'autostart-reconcile') {
    await sleep(3000) // the startup reconcile runs in the background
    await control('autostart-state:after-startup')
  } else if (phase === 'autostart-rollback') {
    await set(
      'disable-fails',
      false,
      r => r.status === 'error' && String(r.error.message).includes('Failed to apply OS autostart')
    )
    await control('autostart-state:after-failure')
  }
  await control('exit')
}

// Tray device-sync submenu with a paired peer, the localized menu, the notification bridge and the
// lightweight-mode exit.
// Observes the devices list the user sees while the orchestrator drives a peer's group key update through the
// Engine. Read-only: it reports every change of a peer row's status and the selected peer's status description,
// so the orchestrator can assert the UI follows the daemon's HTTP/WS state without any UI-side retry.
async function runDeviceGroupUpdateScenario() {
  await reachMainLayout()
  const click = (href: string) => () =>
    void waitFor(`link ${href}`, () => navControl(href)).then(a => a.click())
  await navigate(click('/devices'), '/', devicesPage, 'devices', true)
  daemonWs.subscribe(['device-trust', 'system'], event => {
    void record('device-trust-ws', true, { type: event.eventType })
  })
  // A hidden e2e window may have its timers throttled, so report from DOM mutations instead of polling.
  const seen = new Map<string, string>()
  let described = ''
  const report = () => {
    for (const row of document.querySelectorAll<HTMLElement>('[data-testid^="device-peer-"]')) {
      const deviceId = row.dataset.testid!.slice('device-peer-'.length)
      const status = row.dataset.status ?? ''
      const label = row.getAttribute('aria-label') ?? ''
      if (seen.get(deviceId) !== `${status}|${label}`) {
        seen.set(deviceId, `${status}|${label}`)
        void record('device-row', true, { deviceId, status, label })
      }
      if (status === 'security_update' && row.getAttribute('aria-current') !== 'true') row.click()
    }
    const description = $('[data-testid="device-status-description"]')?.textContent ?? ''
    if (description !== described) {
      described = description
      void record('device-status-description', true, { description })
    }
  }
  new MutationObserver(report).observe(document.body, {
    subtree: true,
    childList: true,
    attributes: true,
    characterData: true,
  })
  report()
}

async function runTrayDevicesScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  // The app sets the tray language from its UI language at startup (once or more, and later on a host whose language is not English);
  // wait until those calls went quiet, otherwise the English pin below is overwritten (17c15 base1: the menu read Chinese on a zh-Hans host).
  // JS heartbeat, not awaited: it shows whether this page's timers and its host calls keep running while the scenario waits.
  let beat = 0
  setInterval(() => {
    void record('tray-driver-heartbeat', true, {
      n: ++beat,
      t: Math.round(performance.now()),
      visibility: document.visibilityState,
      focus: document.hasFocus(),
    })
  }, 1000)
  await record('tray-driver-progress', true, 'before-quiet')
  await control('tray-language-quiet:5000')
  await record('tray-driver-progress', true, 'after-quiet')
  await record('tray-driver-progress', true, 'call-start')
  const pending = settle(HostService.SetTrayLanguage('en'))
  void sleep(3000).then(() => record('tray-driver-progress', true, 'call-pending-after-3s'))
  const english = await pending
  await record('tray-driver-progress', true, 'call-returned')
  await record('tray-language-en', english.status === 'ok')
  await control('tray-menu:initial')
  await control('tray-devices-wait:tray-peer-b')
  await control('tray-device-click:tray-peer-b')
  await control('tray-device-click:tray-peer-b')
  const language = await settle(HostService.SetTrayLanguage('zh-CN'))
  await record('tray-language-set', language.status === 'ok')
  await control('tray-menu:zh')
  const granted = await isPermissionGranted()
  const requested = await requestPermission()
  sendNotification({
    id: 21021,
    title: 'Device trust',
    body: 'Needs a decision',
  })
  await sleep(500)
  await record('notification-bridge', granted === true && requested === 'granted')
  await control('tray-lightweight')
  await sleep(10000)
}

// Stores the launch preferences for the next launch (a full quit then stops the daemon, so the next
// launch is a cold start).
async function runStartupSetScenario(phase: string) {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const [, mode, restore] = phase.split(':')
  const patch = {
    general: {
      startupMode: mode as 'normal' | 'silent' | 'lightweight',
      restoreLastEntryOnStartup: restore === 'restore',
    },
  } as never
  // The app initializes the daemon client while it boots; retry until it is ready.
  let saved: { success: boolean } | undefined
  for (let attempt = 0; attempt < 120 && !saved; attempt++) {
    saved = await updateSettings(patch).catch(() => undefined)
    if (!saved) await sleep(500)
  }
  if (!saved) throw new Error('settings could not be saved')
  await record('startup-saved', saved.success, {
    mode,
    restore: restore === 'restore',
  })
  await control('exit')
}

async function installedBundle(): Promise<boolean> {
  const rows = (await Call.ByName('main.EvidenceService.Installed')) as boolean
  return rows
}

run().catch(error =>
  record('driver-error', false, {
    error: String(error),
    path: location.pathname,
    consoleErrors: consoleErrors.slice(-5),
    links: [...document.querySelectorAll('a')].map(a => a.getAttribute('href')),
    text: document.body.innerText.slice(0, 300),
  })
)

// Exercise installed-package detection and the actual Settings update dialog.
async function runLinuxPackageUpdateScenario() {
  const create = await waitFor('setup entry', () => $('[data-testid="setup-entry-create"]'))
  create.click()
  await waitFor('device form', () => $('#device-name'))
  fill('#device-name', 'package-ui-fixture')
  fill('#pass1', SETUP_PASSPHRASE)
  fill('#pass2', SETUP_PASSPHRASE)
  await sleep(200)
  $('[data-testid="setup-initialize-submit"]')!.click()
  await waitFor('setup complete', () => $('[data-testid="setup-complete-later"]') || mainLayout())
  $('[data-testid="setup-complete-later"]')?.click()
  await waitFor('initialised main layout', mainLayout, 120000)
  // A synthetic profile must never report: opt out through the settings API, as the removed first-run notice did.
  await updateSettings({ general: { telemetryEnabled: false, usageAnalyticsEnabled: false } })
  const kind = await settle(HostService.GetInstallKind())
  await record('package-install-kind', kind.status === 'ok', kind)
  if (
    kind.status !== 'ok' ||
    (kind.data !== InstallKind.InstallKindDeb && kind.data !== InstallKind.InstallKindRPM)
  ) {
    throw new Error('expected an installed deb or rpm')
  }
  await navigate(
    () => navControl('/settings')?.click(),
    '/settings',
    () => settingsPageOpen(),
    'package-settings',
    true
  )
  const button = (text: string) =>
    Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      b => b.textContent?.trim() === text
    )
  const about = await waitFor('About category', () => button(i18n.t('settings.categories.about')))
  about.click()
  const check = await waitFor('check update button', () => {
    const b = button(i18n.t('settings.sections.about.checkUpdate'))
    return b && !b.disabled ? b : null
  })
  check.click()
  const dialog = await waitFor('package manager update dialog', () =>
    Array.from(document.querySelectorAll<HTMLElement>('[role="alertdialog"]')).find(
      d =>
        d.textContent?.includes(i18n.t('update.packageManager.title')) &&
        d.querySelector('.font-mono')
    )
  )
  const text = dialog.textContent ?? ''
  const command = dialog.querySelector('.font-mono')?.textContent ?? ''
  const expected =
    kind.data === 'deb'
      ? 'sudo apt update && sudo apt install --only-upgrade uniclipboard'
      : 'sudo dnf upgrade uniclipboard'
  await record('package-update-hint', command === expected, {
    kind: kind.data,
    command,
    expected,
    text,
  })
  sendNotification({
    title: 'Linux acceptance',
    body: 'Isolated package notification fixture',
  })
  await record('package-notification-requested', true)
  await sleep(4000)
  await control('exit')
}

// The generated host contract, called from the real WebView through the same wrapper the pages use: connection and
// identity, typed errors and their severity, argument handling, typed events, and a real daemon restart.
async function runHostContractScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const expected = (error: unknown) => isExpectedCommandError(error)

  // ---- connection and identity (results the daemon owns pass through untouched)
  const info = await settle(HostService.GetDaemonConnectionInfo())
  await record(
    'connection-info',
    info.status === 'ok' &&
      !!info.data &&
      /^http:\/\/127\.0\.0\.1:\d+$/.test(info.data.baseUrl) &&
      info.data.wsUrl.startsWith('ws://'),
    { baseUrl: info.status === 'ok' ? info.data?.baseUrl : null }
  )
  const session = await settle(HostService.GetDaemonSession())
  await record(
    'daemon-session',
    session.status === 'ok' &&
      !!session.data &&
      session.data.sessionToken.length > 0 &&
      session.data.expiresInSecs > 0,
    {
      tokenLength: session.status === 'ok' ? session.data?.sessionToken.length : null,
    }
  )
  const id = await commands.getDeviceID()
  const meta = await commands.getDeviceMeta()
  await record(
    'device-identity',
    id.length > 0 && meta.deviceId === id && meta.platform === expectedSkin(),
    {
      platform: meta.platform,
      appChannel: meta.appChannel,
      runtimeProfile: meta.runtimeProfile,
    }
  )
  const recovery = await settle(HostService.GetProfileRecovery())
  await record(
    'profile-recovery-passthrough',
    recovery.status === 'ok' && typeof recovery.data === 'object',
    {
      keys: recovery.status === 'ok' && recovery.data ? Object.keys(recovery.data).sort() : null,
    }
  )
  const startup = await settle(HostService.GetDaemonStartupStatus())
  // A healthy daemon no longer serves /startup (it only does while starting), which surfaces as a system error.
  await record(
    'startup-status-passthrough',
    startup.status === 'ok' || (startup.error as { code?: string }).code === 'InternalError',
    startup
  )
  const unlocked = await commands.getContentUnlocked()
  await record('content-unlocked-boolean', typeof unlocked === 'boolean', {
    unlocked,
  })
  const kind = await commands.getInstallKind()
  await record('install-kind-enum', Object.values(InstallKind).includes(kind), {
    kind,
  })
  await record('bootstrap-failure-null', (await commands.getDaemonBootstrapFailure()) === null)

  // ---- typed event: the host emits a snapshot, the page receives the generated payload type
  const seen: EffectsSnapshot[] = []
  const offEffects = Events.On(
    'visual-effects://changed',
    event => void seen.push(event.data as EffectsSnapshot)
  )
  const snapshot = await commands.setVisualEffectsMode(EffectsMode.EffectsModeSmooth)
  await waitFor('visual-effects event', () => seen.length > 0)
  offEffects()
  await record(
    'visual-effects-typed-event',
    seen[0].mode === EffectsMode.EffectsModeSmooth &&
      seen[0].revision === snapshot.revision &&
      seen[0].sessionId === snapshot.sessionId,
    { revision: seen[0].revision, mode: seen[0].mode }
  )
  const theme = await commands.setFollowOmarchyTheme(true)
  await record(
    'desktop-theme-unavailable',
    theme.omarchyAvailable === false &&
      (await commands.getDesktopTheme()).omarchyAvailable === false
  )

  // ---- errors: business failures are user-facing, system failures are not
  const bogusMode = await settle(HostService.SetVisualEffectsMode('bogus' as EffectsMode))
  await record(
    'error-validation-user-facing',
    bogusMode.status === 'error' &&
      (bogusMode.error as { code?: string }).code === 'ValidationError' &&
      expected(bogusMode.error),
    bogusMode
  )
  const missing = await settle(commands.revealPath('/definitely/not/here'))
  await record(
    'error-not-found-user-facing',
    missing.status === 'error' &&
      (missing.error as { code?: string }).code === 'NotFound' &&
      expected(missing.error),
    missing
  )
  const nullPath = await settle(HostService.RevealPath(null as unknown as string))
  await record(
    'null-argument-is-zero-value',
    nullPath.status === 'error' && (nullPath.error as { code?: string }).code === 'NotFound',
    nullPath
  )
  const noPending = await settle(commands.installUpdate())
  await record(
    'error-text-is-system',
    noPending.status === 'error' &&
      typeof noPending.error === 'string' &&
      !expected(noPending.error),
    noPending
  )
  const arity = await settle(Call.ByName('main.HostService.RevealPath'))
  await record(
    'error-wrong-arity-is-system',
    arity.status === 'error' && !arity.raw.hasCause && !expected(arity.error),
    arity.raw
  )
  const unknownMethod = await settle(Call.ByName('main.HostService.NoSuchCommand'))
  await record(
    'error-unknown-method-is-system',
    unknownMethod.status === 'error' && !expected(unknownMethod.error),
    unknownMethod.raw
  )
  const badBytes = await settle(HostService.SaveImageAs('a.png', 'not*base64'))
  await record(
    'error-bad-base64-is-system',
    badBytes.status === 'error' && !expected(badBytes.error),
    badBytes.raw
  )

  // ---- a real daemon restart: events in order, and the replaced client reaches the new process
  const shuttingDown: number[] = []
  const reconnect: number[] = []
  const offDown = Events.On('app://shutting-down', () => void shuttingDown.push(Date.now()))
  const offChanged = Events.On(
    'app://daemon-connection-changed',
    () => void reconnect.push(Date.now())
  )
  await record('restart-daemon-start', true, { deviceId: id })
  const restart = await settle(HostService.RestartDaemon())
  await record('restart-daemon-result', restart.status === 'ok', restart)
  await waitFor('restart events', () => shuttingDown.length > 0 && reconnect.length > 0, 20000)
  offDown()
  offChanged()
  await record(
    'restart-daemon-events',
    shuttingDown.length === 1 && reconnect.length === 1 && shuttingDown[0] <= reconnect[0],
    {
      shuttingDown: shuttingDown.length,
      reconnect: reconnect.length,
    }
  )
  const afterId = await settle(HostService.GetDeviceID())
  await record(
    'restart-daemon-client-replaced',
    afterId.status === 'ok' && afterId.data === id,
    afterId
  )
  const afterSession = await settle(HostService.GetDaemonSession())
  await record(
    'restart-daemon-new-session',
    afterSession.status === 'ok' && (afterSession.data?.sessionToken.length ?? 0) > 0
  )
  await control('exit')
}

// Cancelling a running download: the progress events arrive in order, the call rejects, the pending update is
// available again, and a second download completes. (Cancelling the Wails call itself does not stop the
// download by design: it continues in the background and `cancel_download` is the explicit cancel.)
async function runDownloadCancelScenario() {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const events: DownloadEvent[] = []
  const off = Events.On(
    'update-download-progress',
    event => void events.push(event.data as DownloadEvent)
  )
  const found = await settle(HostService.CheckForUpdate(null))
  await record('cancel-check-found-update', found.status === 'ok' && !!found.data?.version, found)
  const first = settle(HostService.DownloadUpdate())
  await waitFor(
    'first progress event',
    () => events.some(e => e.event === DownloadEventKind.DownloadEventProgress),
    60000
  )
  await record(
    'cancel-progress-started',
    events[0]?.event === DownloadEventKind.DownloadEventStarted,
    { first: events[0] }
  )
  const mid = await HostService.GetDownloadProgress()
  await record(
    'cancel-phase-downloading',
    mid.phase === DownloadPhase.DownloadPhaseDownloading,
    mid
  )
  await HostService.CancelDownload()
  const cancelled = await first
  await record(
    'cancel-call-rejected',
    cancelled.status === 'error' && typeof cancelled.error === 'string',
    cancelled
  )
  await waitFor(
    'failed event',
    () => events.some(e => e.event === DownloadEventKind.DownloadEventFailed),
    10000
  )
  const after = await HostService.GetDownloadProgress()
  await record(
    'cancel-phase-available-again',
    after.phase === DownloadPhase.DownloadPhaseAvailable,
    after
  )
  const second = await settle(HostService.DownloadUpdate())
  await record('cancel-second-download-completes', second.status === 'ok', second)
  const finished = await HostService.GetDownloadProgress()
  const sequence = events.map(e => e.event)
  off()
  await record('cancel-phase-ready', finished.phase === DownloadPhase.DownloadPhaseReady, finished)
  await record(
    'cancel-event-order',
    sequence[0] === DownloadEventKind.DownloadEventStarted &&
      sequence.includes(DownloadEventKind.DownloadEventFailed),
    {
      kinds: [...new Set(sequence)],
    }
  )
  await control('exit')
}
