import { pendingStartupSnapshot } from '@/lib/startup-progress'

// The complete desktop frontend (`@/bootstrap`: App, routing, layout, sidebar,
// toolbar search, list, preview, daemon HTTP + WebSocket) in a plain browser
// against a REAL isolated daemon. Only the Tauri native layer is stubbed: it
// hands over the daemon address and a GUI session token, answers window/theme
// queries with neutral values, and records every native command it receives
// in `window.__ucNativeCalls` so a run shows exactly what was not native.
const params = new URLSearchParams(location.search)
const baseUrl = params.get('daemon') ?? ''
const sessionToken = params.get('token') ?? ''
const calls: { command: string; handled: boolean }[] = []
const errors: string[] = []
Object.assign(window, { __ucNativeCalls: calls, __ucPageErrors: errors })
window.addEventListener('error', e => errors.push(`error: ${e.message}`))
window.addEventListener('unhandledrejection', e => errors.push(`rejection: ${String(e.reason)}`))

// Native commands that only proxy daemon state are answered from the same
// daemon, so the page sees real values.
async function daemonData(path: string) {
  const url = new URL(`${baseUrl}${path}`)
  url.searchParams.set('auth', `Session ${sessionToken}`)
  const res = await fetch(url)
  if (!res.ok) throw new Error(`fixture: ${path} -> ${res.status}`)
  return (await res.json()).data
}

let nextCallbackId = 1
const native: Record<string, (args: Record<string, unknown>) => unknown> = {
  // Host-only state: the native host reports the daemon it supervises as
  // ready; here the daemon is the test's, already up. Version is the daemon's.
  get_daemon_startup_status: async () => ({
    package_version: (await daemonData('/health')).package_version,
    service_ready: true,
    service_failed: false,
    progress: pendingStartupSnapshot,
  }),
  get_profile_recovery: () => daemonData('/encryption/recovery'),
  get_content_unlocked: async () => (await daemonData('/content-lock')).unlocked,
  get_daemon_connection_info: () => ({
    baseUrl,
    wsUrl: `${baseUrl.replace(/^http/, 'ws')}/ws`,
  }),
  get_daemon_session: () => ({ sessionToken, expiresInSecs: 300, refreshAtSecs: 240 }),
  'plugin:event|listen': () => nextCallbackId++,
  'plugin:event|unlisten': () => null,
  'plugin:event|emit': () => null,
}
Object.defineProperty(window, '__TAURI_INTERNALS__', {
  configurable: true,
  value: {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    transformCallback: () => nextCallbackId++,
    unregisterCallback: () => {},
    convertFileSrc: (path: string) => path,
    invoke: async (command: string, args: Record<string, unknown> = {}) => {
      const handler = native[command]
      calls.push({ command, handled: Boolean(handler) })
      return handler ? handler(args) : null
    },
  },
})

Object.defineProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', {
  configurable: true,
  value: { unregisterListener: () => {} },
})

history.replaceState(null, '', '/history')
void import('@/bootstrap')
