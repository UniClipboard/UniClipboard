import { attachConsole } from '@tauri-apps/plugin-log'
import React from 'react'
import ReactDOM from 'react-dom/client'
import { Provider } from 'react-redux'
import App from './App'
import './i18n'
import { store } from './store'
import { getDeviceMeta } from '@/api/runtime'
import { connectDaemonWs, registerDaemonShutdownListener } from '@/lib/daemon-ws-bootstrap'
import { applyDeviceMetaToSentry, initSentry, Sentry } from '@/observability/sentry'

// Sentry init runs before React mounts so that the global ErrorBoundary,
// the pino → Sentry.logger transmit hook, and breadcrumb capture are all
// wired up by the time any module calls `createLogger()`. Whether logs
// actually leave the process is gated at runtime by
// `setFrontendSentryEnabled`, which SettingContext flips once the daemon
// returns the persisted user preference.
initSentry()

// Fire-and-forget: 启动后第一时间把 Rust 侧解析好的 device + app meta
// 拉过来,推进 Sentry 全局 scope。两端事件因此共享同一组 device.id /
// app.version / app.channel tag,在 Sentry UI 上可以按 device.id join 出
// "A 发送 → B 接收"的完整链路,这是 PR1 的核心目标。
// 失败只 console.warn:Tauri runtime 还没就绪 / meta 未生成都不应阻塞渲染。
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

const applyPlatformTypographyScale = () => {
  if (typeof navigator === 'undefined' || typeof document === 'undefined') {
    return
  }

  const ua = navigator.userAgent || ''
  const isWindows = ua.includes('Windows')

  if (!isWindows) {
    return
  }

  const root = document.documentElement

  root.style.setProperty('--font-size-caption', '0.6875rem') /* 11px */
  root.style.setProperty('--font-size-small', '0.75rem') /* 12px */
  root.style.setProperty('--font-size-body', '0.8125rem') /* 13px */
  root.style.setProperty('--font-size-body-lg', '0.875rem') /* 14px */
  root.style.setProperty('--font-size-section', '0.9375rem') /* 15px */
  root.style.setProperty('--font-size-title', '1.125rem') /* 18px */
}

applyPlatformTypographyScale()

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
