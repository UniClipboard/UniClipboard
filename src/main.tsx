import { attachConsole } from '@tauri-apps/plugin-log'
import React from 'react'
import ReactDOM from 'react-dom/client'
import { Provider } from 'react-redux'
import App from './App'
import './i18n'
import { store } from './store'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { createLogger } from '@/lib/logger'
import { initializeWindowUi } from '@/lib/window-ui'
import { initFrontendOtlp } from '@/observability/otlp'
import { initSentry, Sentry } from '@/observability/sentry'

const log = createLogger('main')

initSentry()
initFrontendOtlp()
initializeWindowUi()

const startupTimingOrigin = Date.now()
const logStartupTiming = (label: string) => {
  const elapsed = Date.now() - startupTimingOrigin
  log.debug({ elapsed }, `[StartupTiming] ${label}`)
}

logStartupTiming('main.tsx module init')

if (typeof window !== 'undefined') {
  window.addEventListener('DOMContentLoaded', () => {
    logStartupTiming('DOMContentLoaded')
  })
  window.addEventListener('load', () => {
    logStartupTiming('window load')
  })
}

// Forward Rust/Tauri backend logs to the browser DevTools console.
const initLogging = async () => {
  try {
    if (typeof window !== 'undefined' && '__TAURI__' in window) {
      await attachConsole()
      log.debug('Tauri console attached')
    }
  } catch (error) {
    log.error({ err: error }, 'Failed to attach Tauri console')
  }
}

initLogging().then(() => {
  log.debug('Logging system initialized')
})

// Connect the frontend WebSocket client to the daemon.
// This must run before React renders so that daemonWs is connected by the time
// hooks (useEncryptionState, usePairingEvents, useClipboardNewContent) mount.
connectDaemonWs().catch(err => {
  log.error({ err }, 'daemon WS bootstrap failed')
})

logStartupTiming('ReactDOM.render invoked')

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <Provider store={store}>
      <Sentry.ErrorBoundary fallback={<div>Something went wrong.</div>}>
        <App />
      </Sentry.ErrorBoundary>
    </Provider>
  </React.StrictMode>
)
