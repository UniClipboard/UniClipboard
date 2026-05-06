import * as Sentry from '@sentry/react'
import pino from 'pino'
import type { LogEvent } from 'pino'
import { redactSensitiveArgs } from '@/observability/redaction'
import { traceManager } from '@/observability/trace'

type SentryLogFn = (message: string, attributes?: Record<string, unknown>) => void

const SENTRY_LOG_FN: Record<string, SentryLogFn> = {
  trace: Sentry.logger.trace,
  debug: Sentry.logger.debug,
  info: Sentry.logger.info,
  warn: Sentry.logger.warn,
  error: Sentry.logger.error,
  fatal: Sentry.logger.fatal,
}

function stringifyArg(value: unknown, includeStack = false): string {
  if (value instanceof Error) {
    const base = `${value.name}: ${value.message}`
    return includeStack && value.stack ? value.stack : base
  }
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean' || value == null) return String(value)
  try {
    return JSON.stringify(redactSensitiveArgs(value))
  } catch {
    return String(value)
  }
}

function transmitToSentry(level: string, logEvent: LogEvent): void {
  const send = SENTRY_LOG_FN[level] ?? Sentry.logger.info

  // Build message from all arguments, applying redaction on each.
  const message = logEvent.messages.map(m => stringifyArg(redactSensitiveArgs(m), false)).join(' ')

  // Merge all child-logger bindings (e.g. { module: 'api' }) into a flat object.
  const context = Object.assign({}, ...logEvent.bindings) as Record<string, unknown>

  const traceId = traceManager.getCurrentTrace()?.traceId
  const attributes: Record<string, unknown> = {}
  if (traceId) attributes.trace_id = traceId
  if (context.module) attributes.module = String(context.module)

  send(message, Object.keys(attributes).length > 0 ? attributes : undefined)
}

/**
 * Application-wide pino logger.
 *
 * - In development: writes to browser DevTools console (default pino/browser
 *   behaviour) and forwards structured records to Sentry Logs (gated at
 *   runtime by `setFrontendSentryEnabled`).
 * - In production: console output is suppressed below 'warn'; Sentry receives
 *   all records at 'info' and above.
 *
 * Prefer creating module-level child loggers via `createLogger('module-name')`
 * for structured context rather than adding prefix strings to messages.
 *
 * @example
 * ```ts
 * const log = createLogger('daemon-ws')
 * log.info('connected')
 * log.warn({ attempt: 3 }, 'reconnecting')
 * log.error({ err }, 'fatal connection error')
 * ```
 */
export const logger = pino({
  level: import.meta.env.DEV ? 'debug' : 'info',
  browser: {
    transmit: {
      level: 'info',
      send: transmitToSentry,
    },
  },
})

/**
 * Create a child logger bound to a named module.
 * The `module` field is forwarded as a Sentry log attribute so logs can be
 * filtered by component in the Sentry Logs UI.
 */
export function createLogger(module: string): pino.Logger {
  return logger.child({ module })
}
