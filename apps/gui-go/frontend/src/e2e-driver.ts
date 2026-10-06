// E2E-only scenario driver. It runs inside the real Wails WebView, interacts
// with the shared React DOM and reports each assertion to the native test
// service. It is bundled only when VITE_GUI_GO_E2E=1.
import { Call } from '@wailsio/runtime'
import { daemonClient } from '@/api/daemon/client'
import { setQuickPanelEnabled, setQuickPanelPosition } from '@/api/tauri-command/settings'
import { daemonWs } from '@/lib/daemon-ws'
import { commands } from '@/lib/ipc-bindings.generated'

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
  Call.ByName('main.EvidenceService.Record', { window: windowName, step, ok, detail })
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
  if (phase.startsWith('update')) return runUpdateScenario(phase)
  if (phase === 'file-preview') return runFilePreviewScenario()
  if (phase === 'scheduler') return runSchedulerScenario()
  if (phase === 'quick-panel-settings') return runQuickPanelSettingsScenario()
  if (phase === 'native-panel') return runNativePanelScenario()
  if (phase === 'file-ops') return runFileOpsScenario()
  if (phase.startsWith('autostart')) return runAutostartScenario(phase)
  if (phase === 'config-export') return runConfigExportScenario()
  if (phase === 'config-applied') {
    await waitFor('app root content', () => document.getElementById('root')?.children.length)
    await control('prefs')
    return control('exit')
  }
  if (phase === 'native-requests') return runNativeRequestsScenario()
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
  const pid = await Call.ByName('main.HostService.Invoke', 'get_tauri_pid', {})
  await record('webview-alive-after-reopen', !!(pid as { ok: boolean }).ok && !!link('/settings'))
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
    const response = await daemonClient.request<{ data: Array<{ preview: string }> }>(
      '/clipboard/entries?limit=50&offset=0'
    )
    const found = response.data
      .flatMap(entry => entry.preview.split('\n'))
      .filter(line => /^file:\/\//i.test(line.trim()))
    if (found.length > 0) return found
    await sleep(1000)
  }
  throw new Error('timeout: file entry in history')
}

// A received image file must preview through the host route; a locked or unknown path must not.
async function runFilePreviewScenario() {
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
  const refused = await commands.setQuickPanelDoubleTapModifier('alt', null)
  await record(
    'double-tap-unavailable-rejected',
    refused.status === 'error' && refused.error.code === 'Conflict',
    {
      result: refused,
    }
  )
  const accepted = await commands.setQuickPanelDoubleTapModifier('disabled', null)
  await record('double-tap-disabled-accepted', accepted.status === 'ok', { result: accepted })
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
  const shortcut = await commands.updateKeyboardShortcuts(
    { 'global.toggleQuickPanel': 'Ctrl+Alt+Space' },
    null
  )
  await record('shortcut-saved', shortcut.status === 'ok', { result: shortcut })
  await sleep(4000)
  await record('act-double-tap', true)
  const tap = await commands.setQuickPanelDoubleTapModifier('alt', null)
  await record('double-tap-saved', tap.status === 'ok', { result: tap })
  const availability = await commands.getQuickPanelDoubleTapAvailability(null)
  await record('double-tap-availability', availability.status === 'ok', { result: availability })
  await sleep(4000)
  await record('act-kill', true) // the orchestrator kills the helper now; it must come back
  await sleep(9000)
  await record('act-exit', true)
  await control('exit')
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
  const png = [137, 80, 78, 71, 13, 10, 26, 10, 0, 1, 2, 3]
  const picked = await commands.pickDirectory(null)
  await record('pick-directory-chosen', picked.status === 'ok', { result: picked })
  await result('pick-directory-cancelled', commands.pickDirectory(null))
  await result('save-image-cancelled', commands.saveImageAs('a.png', png, null))
  await result('save-image-saved', commands.saveImageAs('../../x/shot.png', png, null))
  await result('open-image-first', commands.openImageExternally('../../etc/first.png', png, null))
  await result(
    'open-image-second',
    commands.openImageExternally('second.png', png.slice(0, 4), null)
  )
  await result('open-data-directory', commands.openDataDirectory(null))
  await result('open-logs-directory', commands.openLogsDirectory(null))
  const existing = picked.status === 'ok' && picked.data ? picked.data : ''
  await result('reveal-existing', commands.revealPath(existing, null))
  await result(
    'reveal-missing',
    commands.revealPath('/definitely/not/here', null),
    r => r.status === 'error' && r.error.code === 'NotFound'
  )
  await result('export-logs-cancelled', commands.exportStartupLogs(null))
  await result('export-logs-saved', commands.exportStartupLogs(null))
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
  const exported = await step('export', commands.exportConfigPackage(null), r => r.status === 'ok')
  const bundle = exported.data?.path as string
  await step(
    'export-cancelled',
    commands.exportConfigPackage(null),
    r => r.status === 'error' && r.error.kind === 'cancelled'
  )
  const picked = await step(
    'pick-bundle',
    commands.pickConfigBundlePath(null),
    r => r.status === 'ok' && r.data === bundle
  )
  await step(
    'pick-bundle-cancelled',
    commands.pickConfigBundlePath(null),
    r => r.status === 'ok' && r.data === null
  )
  await step(
    'preview-wrong-password',
    commands.previewConfigImport('definitely-wrong', picked.data, null),
    r => r.status === 'error' && r.error.kind === 'daemon'
  )
  await step(
    'preview',
    commands.previewConfigImport(passphrase, bundle, null),
    r => r.status === 'ok' && !!r.data.profileId && !!r.data.appVersion
  )
  await setQuickPanelPosition('center')
  await step(
    'import-staged',
    commands.importConfigPackage(passphrase, bundle, null),
    r => r.status === 'ok' && r.data.stagedOk === true
  )
  await control('exit')
}

// Launch-at-login: the preference and the OS login item must move together, and a failed OS change must
// roll the preference back. Each phase is one launch; the orchestrator prepares the disk in between.
async function runAutostartScenario(phase: string) {
  await waitFor('app root content', () => document.getElementById('root')?.children.length)
  const set = async (step: string, enabled: boolean, accept: (r: any) => boolean) => {
    const r = await commands.updateAutostart(enabled, null)
    await record(step, accept(r), { result: r })
  }
  if (phase === 'autostart') {
    await set('enable', true, r => r.status === 'ok')
    await control('autostart-state:enabled')
    await set('disable', false, r => r.status === 'ok')
    await control('autostart-state:disabled')
    await set('enable-again', true, r => r.status === 'ok')
    await control('autostart-state:enabled-again')
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
