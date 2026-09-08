import { daemonClient } from '@/api/daemon/client'
import { visualEffectsApi } from '@/api/visual-effects'
import { INITIAL_EFFECTS, initializeVisualEffects } from '@/lib/visual-effects-store'

/** Isolate visual checks from the user's daemon, credentials and external services. */
export function installSettingsFixtureEnvironment() {
  let callbackId = 0
  Object.defineProperty(window, '__TAURI_INTERNALS__', {
    configurable: true,
    value: {
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
      transformCallback: () => ++callbackId,
      unregisterCallback: () => {},
      invoke: async (command: string) => {
        if (command === 'plugin:app|version') return '1.0.0'
        if (command === 'get_daemon_session')
          return { sessionToken: 'visual-fixture', expiresInSecs: 3600 }
        if (command === 'get_quick_panel_double_tap_availability') return 'supported'
        if (command === 'plugin:event|listen') return callbackId
        if (command === 'plugin:event|unlisten' || command === 'plugin:webview|set_webview_zoom')
          return null
        throw new Error(`No fixture for native command: ${command}`)
      },
    },
  })
  const originalFetch = window.fetch.bind(window)
  window.fetch = async (request, options) => {
    const url = new URL(request instanceof Request ? request.url : String(request), location.href)
    if (url.pathname === '/api/v1/sponsors' && url.hostname === 'www.uniclipboard.app') {
      return Response.json({
        items: [{ id: 'demo', name: 'Demo supporter', tier: 'regular' }],
        count: 1,
      })
    }
    if (url.pathname.startsWith('/fixture-daemon/')) {
      const responses: Record<string, unknown> = {
        '/fixture-daemon/storage/stats': {
          totalBytes: 52428800,
          databaseBytes: 10485760,
          vaultBytes: 31457280,
          cacheBytes: 8388608,
          logsBytes: 2097152,
        },
        '/fixture-daemon/search/status': {
          state: 'ready',
          reason: null,
          lastRebuildStartedAtMs: null,
          lastRebuildCompletedAtMs: null,
        },
      }
      const data = responses[url.pathname]
      if (data) return Response.json({ data, ts: Date.now() })
      return Response.json(
        { error: 'This action is not connected in the visual fixture.' },
        { status: 400 }
      )
    }
    return originalFetch(request, options)
  }
  daemonClient.initialize({
    baseUrl: `${location.origin}/fixture-daemon`,
    wsUrl: `ws://${location.host}/fixture-daemon/ws`,
  })
  let effects = { ...INITIAL_EFFECTS, sessionId: 'settings-fixture', persistence: 'saved' as const }
  visualEffectsApi.get = async () => effects
  visualEffectsApi.subscribe = async () => () => {}
  visualEffectsApi.environment = async (_, systemMotion) => {
    effects = { ...effects, systemMotion, revision: effects.revision + 1 }
    return effects
  }
  visualEffectsApi.setMode = async mode => {
    effects = {
      ...effects,
      mode,
      lowEffects: mode !== 'effects',
      reduceMotion: mode !== 'effects',
      reason: 'manual',
      revision: effects.revision + 1,
    }
    return effects
  }
  initializeVisualEffects()
}
