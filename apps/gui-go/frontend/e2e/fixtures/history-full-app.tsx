import { pendingStartupSnapshot } from '@/lib/startup-progress'
import { HostRejection, installFakeHost } from './fake-host'

// The complete desktop frontend (`@/bootstrap`: App, routing, layout, sidebar,
// toolbar search, list, preview, daemon HTTP + WebSocket) in a plain browser
// against a REAL isolated daemon. Only the native host is stubbed (a Wails runtime transport, see fake-host.ts): it
// hands over the daemon address and a GUI session token, answers window/theme
// queries with neutral values, and records every native command it receives
// in `window.__ucNativeCalls` so a run shows exactly what was not native.
const params = new URLSearchParams(location.search)
const baseUrl = params.get('daemon') ?? ''
const sessionToken = params.get('token') ?? ''
const errors: string[] = []
Object.assign(window, { __ucPageErrors: errors })
window.addEventListener('error', e => errors.push(`error: ${e.message}`))
window.addEventListener('unhandledrejection', e => errors.push(`rejection: ${String(e.reason)}`))

// Native commands that only proxy daemon state are answered from the same
// daemon, so the page sees real values.
async function daemonData(path: string, body?: unknown) {
  const url = new URL(`${baseUrl}${path}`)
  url.searchParams.set('auth', `Session ${sessionToken}`)
  const res = await fetch(
    url,
    body === undefined
      ? undefined
      : {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        }
  )
  const response = await res.json()
  if (!res.ok) throw new HostRejection({ code: response.code ?? 'INTERNAL' })
  return response.data
}

installFakeHost({
  // Host-only state: the native host reports the daemon it supervises as
  // ready; here the daemon is the test's, already up. Version is the daemon's.
  GetDaemonStartupStatus: async () => ({
    package_version: (await daemonData('/health')).package_version,
    service_ready: true,
    service_failed: false,
    progress: pendingStartupSnapshot,
  }),
  GetProfileRecovery: () => daemonData('/encryption/recovery'),
  GetContentUnlocked: async () => (await daemonData('/content-lock')).unlocked,
  UnlockContent: (request: unknown) => daemonData('/content-lock/unlock', request),
  UnlockContentFromKeyring: async () =>
    params.has('denyKeyring')
      ? false
      : (await daemonData('/content-lock/unlock-keyring', {})).unlocked,
  GetDaemonConnectionInfo: () => ({
    baseUrl,
    wsUrl: `${baseUrl.replace(/^http/, 'ws')}/ws`,
  }),
  GetDaemonSession: () => ({ sessionToken, expiresInSecs: 300, refreshAtSecs: 240 }),
})

// Optional platform branch for checking the Windows/Linux layouts in a browser
// (DOM only, not a native acceptance): ?platform=windows|linux, ?frame=system.
const platformOverride = params.get('platform')
if (platformOverride === 'windows' || platformOverride === 'linux') {
  const windows = platformOverride === 'windows'
  const fake = {
    userAgent: windows
      ? 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0 Safari/537.36'
      : 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0 Safari/537.36',
    platform: windows ? 'Win32' : 'Linux x86_64',
    userAgentData: { platform: windows ? 'Windows' : 'Linux', brands: [], mobile: false },
  }
  for (const [key, value] of Object.entries(fake)) {
    Object.defineProperty(navigator, key, { configurable: true, get: () => value })
  }
}
const frame = params.get('frame')
if (frame) localStorage.setItem('uniclipboard.useSystemWindowFrame', frame)

const quickPanel = params.get('surface') === 'quick-panel'
history.replaceState(null, '', quickPanel ? '/quick-panel.html' : '/history')
if (quickPanel) void import('@/quick-panel/main')
else void import('@/bootstrap')
