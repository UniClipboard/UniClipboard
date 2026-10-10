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
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification'
// E2E-only scenario driver. It runs inside the real Wails WebView, interacts
// with the shared React DOM and reports each assertion to the native test
// service. It is bundled only when VITE_GUI_GO_E2E=1.
import { Call, Events } from '@wailsio/runtime'
import { daemonClient } from '@/api/daemon/client'
import { updateSettings } from '@/api/daemon/settings'
import { setQuickPanelEnabled, setQuickPanelPosition } from '@/api/tauri-command/settings'
import i18n from '@/i18n'
import { daemonWs } from '@/lib/daemon-ws'
import { commands } from '@/lib/ipc'
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
const devicesPage = () => $('[data-testid="devices-add-device"]')
// The history route with the sidebar shell: independent of list content and locale.
const mainLayout = () =>
  location.pathname.startsWith('/history') &&
  link('/settings') &&
  !$('[data-testid="setup-entry-create"]') &&
  !$('#unlock-passphrase') &&
  !$('[data-testid="unlock-content"]')

async function navigate(
  go: () => void,
  href: string,
  marker: () => unknown,
  step: string,
  retryClick = false
) {
  go()
  const arrived = () => location.pathname.startsWith(href) && marker()
  // A click can land while the sidebar is still animating in; retry link clicks until the route changes.
  for (let attempt = 0; retryClick && attempt < 20 && !arrived(); attempt++) {
    await sleep(1000)
    if (!arrived()) go()
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
  if (phase === 'single-image-ui') return runSingleImageUiScenario()
  if (phase === 'scheduler') return runSchedulerScenario()
  if (phase === 'wake') return // the orchestrator drives this launch through the control file
  if (phase === 'quick-panel-settings') return runQuickPanelSettingsScenario()
  if (phase === 'native-panel') return runNativePanelScenario()
  if (phase === 'file-ops') return runFileOpsScenario()
  if (phase === 'startup-observe') return // the native observer reports; the hidden window may not run scripts
  if (phase.startsWith('startup-set:')) return runStartupSetScenario(phase)
  if (phase === 'tray-devices') return runTrayDevicesScenario()
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
      await waitFor('wrong passphrase message', () => document.body.innerText.includes('口令错误'))
      await record('unlock-wrong-passphrase-rejected', true)
      fill('#unlock-passphrase', SETUP_PASSPHRASE)
      ;($('#unlock-passphrase') as HTMLInputElement).form?.requestSubmit()
    }
    await waitFor('unlocked', () => mainLayout())
    await record('unlock-accepted', true, { via: next })
  }

  await waitFor('home', mainLayout)
  await record('home', true)
  const click = (href: string) => () =>
    void waitFor(`link ${href}`, () => link(href)).then(a => a.click())
  // At this window width the shared layout shows devices as a panel of the history route.
  await navigate(click('/devices'), '/', devicesPage, 'devices', true)
  const beforeSettings = location.pathname
  await navigate(
    click('/settings'),
    '/settings',
    () => $('[data-testid="settings-page-header"]'),
    'settings',
    true
  )
  await navigate(
    () => history.back(),
    beforeSettings,
    () => !$('[data-testid="settings-page-header"]'),
    'navigate-back'
  )

  await control('close-main')
  await control('reopen-main')
  // The WebView survived the hide/show cycle with its React state and host bindings intact.
  const alive = await settle(HostService.GetDeviceMeta())
  await record('webview-alive-after-reopen', alive.status === 'ok' && !!link('/settings'))
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
  await record('content-lock-changed-event', lockEvents > 0, { events: lockEvents })
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
  const cards = () =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="history-card"]'))
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
  await waitFor('history list rendered', () => $('[data-testid="history-card"]')) // the orchestrator seeds one entry
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
  const card = await waitFor('history card', () =>
    document.querySelector<HTMLElement>(
      `[data-testid="history-card"][data-entry-id="${entry!.id}"]`
    )
  )
  card.querySelector<HTMLElement>('button')!.click()
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
    HostService.UpdateKeyboardShortcuts({ 'global.toggleQuickPanel': 'Ctrl+Alt+Space' })
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
    () => link('/settings')?.click(),
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
  await record('ui-quick-panel-state', true, { ...readState(), locale: i18n.language })
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
  const consent = await waitFor('telemetry notice', () =>
    Array.from(document.querySelectorAll<HTMLElement>('[role="alertdialog"]')).find(dialog =>
      dialog.textContent?.includes(i18n.t('settings.sections.general.telemetry.notice.title'))
    )
  )
  Array.from(consent.querySelectorAll<HTMLButtonElement>('button'))
    .find(
      b => b.textContent?.trim() === i18n.t('settings.sections.general.telemetry.notice.optOut')
    )!
    .click()
  await waitFor('telemetry notice dismissed', () => !consent.isConnected)
  const kind = await settle(HostService.GetInstallKind())
  await record('package-install-kind', kind.status === 'ok', kind)
  if (
    kind.status !== 'ok' ||
    (kind.data !== InstallKind.InstallKindDeb && kind.data !== InstallKind.InstallKindRPM)
  ) {
    throw new Error('expected an installed deb or rpm')
  }
  await navigate(
    () => link('/settings')?.click(),
    '/settings',
    () => $('[data-testid="settings-page-header"]'),
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
  sendNotification({ title: 'Linux acceptance', body: 'Isolated package notification fixture' })
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
    { tokenLength: session.status === 'ok' ? session.data?.sessionToken.length : null }
  )
  const id = await commands.getDeviceID()
  const meta = await commands.getDeviceMeta()
  await record(
    'device-identity',
    id.length > 0 && meta.deviceId === id && meta.platform === 'macos',
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
  await record('content-unlocked-boolean', typeof unlocked === 'boolean', { unlocked })
  const kind = await commands.getInstallKind()
  await record('install-kind-enum', Object.values(InstallKind).includes(kind), { kind })
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
