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
 * Runtime telemetry gate, mirrors `general.telemetryEnabled`.
 *
 * Default `true` so events emitted before settings finish loading still flow
 * to Sentry — losing the first few hundred ms of startup errors would defeat
 * the point of frontend error tracking. SettingContext flips this to the
 * persisted user preference as soon as the daemon returns settings, and on
 * every subsequent update.
 *
 * Backend equivalent: `tracing_subscriber::init` reads telemetry_enabled
 * from disk and gates Sentry/OTLP at init time (requires restart). The
 * frontend can do better — runtime toggle via beforeSend hooks below.
 */
let sentryRuntimeEnabled = true

export function setFrontendSentryEnabled(enabled: boolean): void {
  sentryRuntimeEnabled = enabled
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
