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

// 屏蔽 WebKit/WebView2 默认右键菜单(Inspect / Reload / 拼写检查),否则用户右键
// 任何文本都会暴露 webview 身份。需要原生右键的地方用 @radix-ui/react-context-menu opt-in。
if (typeof window !== 'undefined') {
  window.addEventListener('contextmenu', e => e.preventDefault())

  // DevTools 用 ⌘⌥I (macOS) / Ctrl+Shift+I (Win/Linux) 打开,补偿被禁用的右键菜单。
  window.addEventListener('keydown', e => {
    const isMac = navigator.platform.toLowerCase().includes('mac')
    const modifier = isMac ? e.metaKey && e.altKey : e.ctrlKey && e.shiftKey
    if (modifier && e.key.toLowerCase() === 'i') {
      e.preventDefault()
      void import('@tauri-apps/api/webviewWindow').then(({ getCurrentWebviewWindow }) => {
        const w = getCurrentWebviewWindow() as unknown as { openDevtools?: () => void }
        w.openDevtools?.()
      })
    }
  })
}

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
