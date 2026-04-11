import pino from 'pino'
import type { LogEvent } from 'pino'
import { queueLogRecord, type OtlpLogRecord } from '@/observability/otlp'
import { redactSensitiveArgs } from '@/observability/redaction'
import { traceManager } from '@/observability/trace'

// Pino level label → OTLP severity (OpenTelemetry Log Data Model)
const SEVERITY_MAP: Record<string, { severityNumber: number; severityText: string }> = {
  trace: { severityNumber: 1, severityText: 'TRACE' },
  debug: { severityNumber: 5, severityText: 'DEBUG' },
  info: { severityNumber: 9, severityText: 'INFO' },
  warn: { severityNumber: 13, severityText: 'WARN' },
  error: { severityNumber: 17, severityText: 'ERROR' },
  fatal: { severityNumber: 21, severityText: 'FATAL' },
}

function toNanoTimestamp(ms: number): string {
  return `${BigInt(ms) * 1_000_000n}`
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

function transmitToOtlp(level: string, logEvent: LogEvent): void {
  const severity = SEVERITY_MAP[level] ?? { severityNumber: 9, severityText: 'INFO' }

  // Build message from all arguments, applying redaction on each
  const message = logEvent.messages.map(m => stringifyArg(redactSensitiveArgs(m), false)).join(' ')

  // Merge all child-logger bindings (e.g. { module: 'api' }) into a flat object
  const context = Object.assign({}, ...logEvent.bindings) as Record<string, unknown>

  const traceId = traceManager.getCurrentTrace()?.traceId
  const attributes: Array<{ key: string; value: { stringValue: string } }> = []

  if (traceId) {
    attributes.push({ key: 'trace_id', value: { stringValue: traceId } })
  }
  if (context.module) {
    attributes.push({ key: 'module', value: { stringValue: String(context.module) } })
  }

  const record: OtlpLogRecord = {
    timeUnixNano: toNanoTimestamp(logEvent.ts),
    severityNumber: severity.severityNumber,
    severityText: severity.severityText,
    body: { stringValue: message },
    attributes: attributes.length > 0 ? attributes : undefined,
  }

  queueLogRecord(record)
}

/**
 * Application-wide pino logger.
 *
 * - In development: writes to browser DevTools console (default pino/browser behaviour)
 *   and transmits structured records to OTLP (if configured).
 * - In production: console output is suppressed below 'warn'; OTLP receives all records
 *   at 'info' and above.
 *
 * Prefer creating module-level child loggers via `createLogger('module-name')` for
 * structured context rather than adding prefix strings to messages.
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
      send: transmitToOtlp,
    },
  },
})

/**
 * Create a child logger bound to a named module.
 * The `module` field is forwarded as an OTLP attribute so logs can be
 * filtered by component in Seq / Grafana Loki / etc.
 */
export function createLogger(module: string): pino.Logger {
  return logger.child({ module })
}
