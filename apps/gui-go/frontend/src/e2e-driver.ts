// E2E-only scenario driver. It runs inside the real Wails WebView, interacts
// with the shared React DOM and reports each assertion to the native test
// service. It is bundled only when VITE_GUI_GO_E2E=1.
import { Call } from '@wailsio/runtime'
import { daemonWs } from '@/lib/daemon-ws'

const windowName = 'main'
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
const mainLayout = () => $('[data-testid="history-search-anchor"]')

async function navigate(go: () => void, href: string, marker: () => unknown, step: string) {
  go()
  await waitFor(step, () => location.pathname.startsWith(href) && marker())
  await record(step, true, { path: location.pathname })
}

async function run() {
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
  await navigate(click('/devices'), '/', devicesPage, 'devices')
  const beforeSettings = location.pathname
  await navigate(
    click('/settings'),
    '/settings',
    () => $('[data-testid="settings-page-header"]'),
    'settings'
  )
  await navigate(
    () => history.back(),
    beforeSettings,
    () => !$('[data-testid="settings-page-header"]'),
    'navigate-back'
  )
  await navigate(click('/history'), '/history', mainLayout, 'history')

  await control('close-main')
  await control('reopen-main')
  // The WebView survived the hide/show cycle with its React state and host bindings intact.
  const pid = await Call.ByName('main.HostService.Invoke', 'get_tauri_pid', {})
  await record('webview-alive-after-reopen', !!(pid as { ok: boolean }).ok && !!mainLayout())
  await record('driver-complete', true)
}

run().catch(error =>
  record('driver-error', false, {
    error: String(error),
    path: location.pathname,
    links: [...document.querySelectorAll('a')].map(a => a.getAttribute('href')),
    text: document.body.innerText.slice(0, 300),
  })
)
