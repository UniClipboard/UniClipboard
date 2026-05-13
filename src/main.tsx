import { attachConsole } from '@tauri-apps/plugin-log'
import React from 'react'
import ReactDOM from 'react-dom/client'
import { Provider } from 'react-redux'
import App from './App'
import './i18n'
import { store } from './store'
import { getDeviceMeta } from '@/api/runtime'
import { connectDaemonWs, registerDaemonShutdownListener } from '@/lib/daemon-ws-bootstrap'
import { initializeWindowUi } from '@/lib/window-ui'
import { applyDeviceMetaToSentry, initSentry, Sentry } from '@/observability/sentry'

// Sentry init runs before React mounts so that the global ErrorBoundary,
// the pino → Sentry.logger transmit hook, and breadcrumb capture are all
// wired up by the time any module calls `createLogger()`. Whether logs
// actually leave the process is gated at runtime by
// `setFrontendSentryEnabled`, which SettingContext flips once the daemon
// returns the persisted user preference.
initSentry()

// 启动后异步拉取 Rust 侧解析的 device/app 元数据，用于推进 Sentry 全局 scope。
// 如果 Tauri runtime 未就绪或 meta 未生成，只记录警告，不阻塞渲染。
getDeviceMeta()
  .then(applyDeviceMetaToSentry)
  .catch(err => {
    console.warn('[sentry] failed to attach device meta:', err)
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

// 初始化日志系统：将后端日志输出到浏览器 DevTools
const initLogging = async () => {
  try {
    // 仅在 Tauri 环境中运行（不在浏览器开发模式中）
    if (typeof window !== 'undefined' && '__TAURI__' in window) {
      await attachConsole()
      console.log('[Tauri Log] Console attached successfully')
    }
  } catch (error) {
    console.error('[Tauri Log] Failed to attach console:', error)
  }
}

// 执行日志初始化
initLogging().then(() => {
  console.log('[Tauri Log] Logging system initialized')
})

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

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <Provider store={store}>
      <Sentry.ErrorBoundary fallback={<div>Something went wrong.</div>}>
        <App />
      </Sentry.ErrorBoundary>
    </Provider>
  </React.StrictMode>
)

logStartupTiming('ReactDOM.render invoked')
