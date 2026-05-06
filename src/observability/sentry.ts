import type { ErrorEvent, EventHint } from '@sentry/core'
import * as Sentry from '@sentry/react'
import React from 'react'
import {
  Routes,
  createRoutesFromChildren,
  matchRoutes,
  useLocation,
  useNavigationType,
} from 'react-router-dom'
import { redactSensitiveArgs } from '@/observability/redaction'

const sentryEnabled = Boolean(import.meta.env.VITE_SENTRY_DSN)

/**
 * localStorage 中镜像 `general.telemetryEnabled` 的键名。
 *
 * 由 SettingContext 的 useEffect 写入；本模块在加载阶段（initSentry 之前）
 * 同步读取，作为 `sentryRuntimeEnabled` 的初始值，让"用户上次关闭遥测"的
 * 偏好在启动早期窗口（daemon RPC 返回前）就能生效，不依赖异步 settings load。
 *
 * 后端等价机制是 `uc-bootstrap::tracing` 在 sentry::init 之前同步读
 * settings.json 后调 `telemetry_gate::set_enabled`，本镜像是前端无法同步
 * 读磁盘下的等价物。
 */
const TELEMETRY_MIRROR_KEY = 'uc.telemetry_enabled'

function readTelemetryMirror(): boolean {
  if (typeof window === 'undefined' || !window.localStorage) {
    return false
  }
  try {
    return window.localStorage.getItem(TELEMETRY_MIRROR_KEY) === 'true'
  } catch {
    return false
  }
}

/**
 * 运行时遥测开关，镜像 `general.telemetryEnabled`。
 *
 * 启动时同步从 localStorage 读取上次确认过的用户偏好：
 * - 没值（首次启动）/ 上次为 false → 默认 `false`，启动早期事件被丢弃。
 * - 上次为 true → `true`，启动早期事件也能上传。
 *
 * SettingContext 在 daemon 返回设置后会再次调 setFrontendSentryEnabled
 * 同步到磁盘真值，并刷新 localStorage 镜像。
 */
let sentryRuntimeEnabled = readTelemetryMirror()

export function setFrontendSentryEnabled(enabled: boolean): void {
  sentryRuntimeEnabled = enabled
  if (typeof window !== 'undefined' && window.localStorage) {
    try {
      window.localStorage.setItem(TELEMETRY_MIRROR_KEY, String(enabled))
    } catch {
      // localStorage 满 / 被禁用时静默失败，仅影响下次启动早期窗口的精度。
    }
  }
}

const getTauriPlatform = (): string => {
  if (typeof window === 'undefined' || !('__TAURI__' in window)) {
    return 'unknown'
  }

  const tauriWindow = window as typeof window & {
    __TAURI__?: { platform?: string }
  }

  return tauriWindow.__TAURI__?.platform ?? 'unknown'
}

export function initSentry(): void {
  if (!sentryEnabled) {
    return
  }

  const beforeSend: (event: ErrorEvent, hint: EventHint) => ErrorEvent | null = (event, _hint) => {
    if (!sentryRuntimeEnabled) {
      return null
    }
    const type = event.exception?.values?.[0]?.type
    if (type === 'ResizeObserver loop limit exceeded') {
      return null
    }
    if (event.extra) {
      event.extra = redactSensitiveArgs(event.extra) as Record<string, unknown>
    }
    return event
  }

  const beforeBreadcrumb = (breadcrumb: Sentry.Breadcrumb): Sentry.Breadcrumb | null => {
    if (!sentryRuntimeEnabled) {
      return null
    }
    if (breadcrumb.data) {
      breadcrumb.data = redactSensitiveArgs(breadcrumb.data) as Record<string, unknown>
    }
    return breadcrumb
  }

  Sentry.init({
    dsn: import.meta.env.VITE_SENTRY_DSN,
    tracesSampleRate: import.meta.env.DEV ? 1.0 : 0.1,
    replaysSessionSampleRate: import.meta.env.DEV ? 1.0 : 0.1,
    replaysOnErrorSampleRate: 1.0,
    environment: import.meta.env.VITE_APP_ENV ?? import.meta.env.MODE,
    release: import.meta.env.VITE_APP_VERSION,
    sendDefaultPii: true,
    enableLogs: true,
    debug: import.meta.env.DEV,
    integrations: [
      Sentry.reactRouterV7BrowserTracingIntegration({
        useEffect: React.useEffect,
        useLocation,
        useNavigationType,
        createRoutesFromChildren,
        matchRoutes,
      }),
      Sentry.replayIntegration(),
      Sentry.consoleLoggingIntegration({ levels: ['log', 'info', 'warn', 'error'] }),
    ],
    beforeSend,
    beforeBreadcrumb,
    beforeSendLog: log => {
      if (!sentryRuntimeEnabled) {
        return null
      }
      if (log.attributes) {
        log.attributes = redactSensitiveArgs(log.attributes) as Record<string, unknown>
      }
      return log
    },
    initialScope: {
      tags: {
        platform: getTauriPlatform(),
      },
    },
  })
}

/**
 * Sentry-instrumented Routes component for React Router v7.
 * Use this instead of `Routes` to get parameterized navigation tracing.
 */
export const SentryRoutes = Sentry.withSentryReactRouterV7Routing(Routes)

export { Sentry, sentryEnabled }
