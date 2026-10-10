import { daemonClient } from '@/api/daemon/client'
import { visualEffectsApi } from '@/api/visual-effects'
import { INITIAL_EFFECTS, initializeVisualEffects } from '@/lib/visual-effects-store'

const BUILT_IN_RELAYS = [
  ['na-east', 'https://use1-1.relay.n0.iroh.link./'],
  ['na-west', 'https://usw1-1.relay.n0.iroh.link./'],
  ['eu', 'https://euc1-1.relay.n0.iroh.link./'],
  ['asia-pacific', 'https://aps1-1.relay.n0.iroh.link./'],
] as const

function builtInOverview(inEffect: boolean) {
  return {
    savedMode: 'builtIn',
    appliedMode: inEffect ? 'builtIn' : null,
    changePending: false,
    entries: BUILT_IN_RELAYS.map(([regionId, url]) => ({
      source: 'builtIn',
      regionId,
      url,
      credentialConfigured: false,
      inEffect,
    })),
  }
}

declare global {
  interface Window {
    /** Scripted `GET /settings/relay-overview` response for browser E2E. */
    __relayOverviewFixture?: { response: unknown; calls: number }
    /** Optional harness binding returning the scripted response or a failure. */
    __relayOverviewScript?: () => Promise<{ response?: unknown; fail?: boolean }>
    __settingsFixtureNative?: {
      restartCalls: number
      restartShouldFail: boolean
    }
  }
}

/** Isolate visual checks from the user's daemon, credentials and external services. */
export function installSettingsFixtureEnvironment() {
  let callbackId = 0
  window.__settingsFixtureNative = {
    restartCalls: 0,
    restartShouldFail: false,
  }
  Object.defineProperty(window, '__UC_DESKTOP_HOST__', {
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
        if (command === 'restart_daemon') {
          const native = window.__settingsFixtureNative!
          native.restartCalls += 1
          return native.restartShouldFail
            ? { status: 'error', error: { code: 'restart_failed', message: 'fixture failure' } }
            : { status: 'ok', data: null }
        }
        if (command === 'plugin:event|listen') return callbackId
        if (command === 'plugin:event|unlisten' || command === 'plugin:webview|set_webview_zoom')
          return null
        throw new Error(`No fixture for native command: ${command}`)
      },
    },
  })
  window.__relayOverviewFixture = { response: builtInOverview(true), calls: 0 }
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
      if (url.pathname === '/fixture-daemon/settings/relay-probe') {
        return Response.json({ data: { kind: 'success', latencyMs: 12 }, ts: Date.now() })
      }
      if (url.pathname === '/fixture-daemon/settings/relay-overview') {
        const scripted = window.__relayOverviewFixture!
        scripted.calls += 1
        // A browser harness may script the response from outside the page.
        const external = await window.__relayOverviewScript?.()
        if (external?.fail) {
          return Response.json({ error: 'relay overview unavailable' }, { status: 500 })
        }
        return Response.json({ data: external?.response ?? scripted.response, ts: Date.now() })
      }
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
  const fixtureOrigin = location.protocol === 'file:' ? 'http://fixture.local' : location.origin
  daemonClient.initialize({
    baseUrl: `${fixtureOrigin}/fixture-daemon`,
    wsUrl: `ws://fixture.local/fixture-daemon/ws`,
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
