import React from 'react'
import { Provider } from 'react-redux'
import { getDeviceMeta } from '@/api/runtime'
import App from '@/App'
import { getAppRoot } from '@/app-root'
import { MainWindowReady } from '@/components/app/MainWindowReady'
import '@/i18n'
import { connectDaemonWs, registerDaemonShutdownListener } from '@/lib/daemon-ws-bootstrap'
import { initializeWebviewContextMenu } from '@/lib/webview-context-menu'
import { initializeWindowFrame } from '@/lib/window-frame-runtime'
import { initializeWindowTheme } from '@/lib/window-theme'
import { initializeWindowUi } from '@/lib/window-ui'
import {
  applyDiagnosticDeviceContext,
  initializeDiagnostics,
  DiagnosticsErrorBoundary,
} from '@/observability/diagnostics'
import { store } from '@/store'

initializeWebviewContextMenu()

// Initialize diagnostics before React mounts. The provider owns SDK setup;
// the persisted diagnostics setting still controls remote delivery.
initializeDiagnostics()

// Attach host context without blocking rendering when the runtime is not ready.
getDeviceMeta()
  .then(applyDiagnosticDeviceContext)
  .catch(err => {
    console.warn('[diagnostics] failed to attach device meta:', err)
  })

const startupTimingOrigin = Date.now()
const logStartupTiming = (label: string) => {
  const elapsed = Date.now() - startupTimingOrigin
  console.log(`[StartupTiming] ${label} t=${elapsed}ms`)
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

initializeWindowUi()
const windowFrameReady = initializeWindowFrame()
const windowThemeReady = initializeWindowTheme()

// Connect the frontend WebSocket client to the daemon.
// This must run before React renders so that daemonWs is connected by the time
// hooks (useEncryptionState, useClipboardNewContent) mount.
connectDaemonWs().catch(err => {
  console.error('[main] daemon WS bootstrap failed:', err)
})

// Listen for the Rust shell's pre-shutdown hint so the WebSocket sends a
// proper close frame before the daemon's axum graceful_shutdown runs —
// otherwise the long-lived /ws handler would block shutdown for the full
// heartbeat timeout (~30s).
registerDaemonShutdownListener().catch(err => {
  console.error('[main] daemon shutdown listener registration failed:', err)
})

void Promise.all([windowFrameReady, windowThemeReady]).then(() => {
  getAppRoot().render(
    <React.StrictMode>
      <Provider store={store}>
        <DiagnosticsErrorBoundary
          fallback={
            <>
              <div>Something went wrong.</div>
              <MainWindowReady />
            </>
          }
        >
          <App />
          <MainWindowReady />
        </DiagnosticsErrorBoundary>
      </Provider>
    </React.StrictMode>
  )
  logStartupTiming('ReactDOM.render invoked')
})
