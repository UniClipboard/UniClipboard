import { redactSensitiveArgs } from '@/observability/redaction'

type ConsoleMethod = 'log' | 'info' | 'warn' | 'error'

type OtlpAnyValue =
  | { stringValue: string }
  | { boolValue: boolean }
  | { intValue: number }
  | { doubleValue: number }

type OtlpKeyValue = {
  key: string
  value: OtlpAnyValue
}

type OtlpLogRecord = {
  timeUnixNano: string
  severityNumber: number
  severityText: string
  body: { stringValue: string }
  attributes?: OtlpKeyValue[]
}

const LEVEL_MAP: Record<ConsoleMethod, { severityNumber: number; severityText: string }> = {
  log: { severityNumber: 9, severityText: 'INFO' },
  info: { severityNumber: 9, severityText: 'INFO' },
  warn: { severityNumber: 13, severityText: 'WARN' },
  error: { severityNumber: 17, severityText: 'ERROR' },
}

const FLUSH_INTERVAL_MS = 1_000
const SERVICE_NAME = 'uniclipboard-frontend'
const SCOPE_NAME = 'uniclipboard.frontend.console'

let buffer: OtlpLogRecord[] = []
let flushTimer: ReturnType<typeof setTimeout> | null = null
let logsEndpoint = ''
let initialized = false
let telemetryEnabled = false
let getTraceId: (() => string | undefined) | null = null

const originalConsole = {
  log: console.log.bind(console),
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
}

function normalizeOtlpLogsEndpoint(raw: string): string {
  const trimmed = raw.trim().replace(/\/+$/, '')
  if (!trimmed) return ''
  if (trimmed.endsWith('/v1/logs')) return trimmed
  if (trimmed.endsWith('/v1/traces')) return `${trimmed.slice(0, -'/v1/traces'.length)}/v1/logs`
  if (trimmed.endsWith('/v1/metrics')) {
    return `${trimmed.slice(0, -'/v1/metrics'.length)}/v1/logs`
  }
  return `${trimmed}/v1/logs`
}

function resolveFrontendOtlpEndpoint(): string {
  const endpoint = import.meta.env.VITE_OTEL_EXPORTER_OTLP_ENDPOINT?.trim()
  if (endpoint) {
    return normalizeOtlpLogsEndpoint(endpoint)
  }

  if (import.meta.env.VITE_SEQ_URL) {
    originalConsole.warn(
      '[OTLP] VITE_SEQ_URL is deprecated and ignored. Use VITE_OTEL_EXPORTER_OTLP_ENDPOINT.'
    )
  }

  return ''
}

function buildResourceAttributes(): OtlpKeyValue[] {
  const attrs: OtlpKeyValue[] = [
    { key: 'service.name', value: { stringValue: SERVICE_NAME } },
    { key: 'deployment.environment', value: { stringValue: import.meta.env.MODE } },
  ]

  if (import.meta.env.VITE_APP_VERSION) {
    attrs.push({
      key: 'service.version',
      value: { stringValue: import.meta.env.VITE_APP_VERSION },
    })
  }

  if (typeof navigator !== 'undefined' && navigator.userAgent) {
    attrs.push({
      key: 'browser.user_agent',
      value: { stringValue: navigator.userAgent },
    })
  }

  return attrs
}

function toNanoTimestamp(valueMs: number): string {
  return `${BigInt(valueMs) * 1_000_000n}`
}

function stringifyArg(value: unknown): string {
  if (value instanceof Error) {
    return value.stack || `${value.name}: ${value.message}`
  }
  if (typeof value === 'string') {
    return value
  }
  if (typeof value === 'number' || typeof value === 'boolean' || value == null) {
    return String(value)
  }
  try {
    return JSON.stringify(redactSensitiveArgs(value))
  } catch {
    return String(value)
  }
}

function buildLogRecord(method: ConsoleMethod, args: unknown[]): OtlpLogRecord {
  const now = Date.now()
  const message = args.map(arg => stringifyArg(redactSensitiveArgs(arg))).join(' ')
  const attributes: OtlpKeyValue[] = []
  const traceId = getTraceId?.()

  if (traceId) {
    attributes.push({ key: 'trace_id', value: { stringValue: traceId } })
  }

  return {
    timeUnixNano: toNanoTimestamp(now),
    severityNumber: LEVEL_MAP[method].severityNumber,
    severityText: LEVEL_MAP[method].severityText,
    body: { stringValue: message },
    attributes: attributes.length > 0 ? attributes : undefined,
  }
}

function buildPayload(logRecords: OtlpLogRecord[]) {
  return {
    resourceLogs: [
      {
        resource: {
          attributes: buildResourceAttributes(),
        },
        scopeLogs: [
          {
            scope: {
              name: SCOPE_NAME,
              version: import.meta.env.VITE_APP_VERSION ?? 'dev',
            },
            logRecords,
          },
        ],
      },
    ],
  }
}

async function flush(): Promise<void> {
  if (!telemetryEnabled || !logsEndpoint || buffer.length === 0) {
    buffer = []
    flushTimer = null
    return
  }

  const payload = buildPayload(buffer)
  buffer = []
  flushTimer = null

  try {
    await fetch(logsEndpoint, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
      keepalive: true,
    })
  } catch (error) {
    originalConsole.warn('[OTLP] flush failed:', error)
  }
}

function scheduleFlush() {
  if (flushTimer) {
    return
  }

  flushTimer = setTimeout(() => {
    void flush()
  }, FLUSH_INTERVAL_MS)
}

function patchConsoleMethod(method: ConsoleMethod) {
  const original = originalConsole[method]
  console[method] = (...args: unknown[]) => {
    original(...args)

    if (!telemetryEnabled || !logsEndpoint) {
      return
    }

    buffer.push(buildLogRecord(method, args))
    scheduleFlush()
  }
}

export function initFrontendOtlp(): void {
  if (initialized) {
    return
  }

  logsEndpoint = resolveFrontendOtlpEndpoint()
  initialized = true

  if (!logsEndpoint) {
    return
  }

  patchConsoleMethod('log')
  patchConsoleMethod('info')
  patchConsoleMethod('warn')
  patchConsoleMethod('error')

  import('@/observability/trace').then(module => {
    getTraceId = () => module.traceManager.getCurrentTrace()?.traceId
  })

  if (typeof window !== 'undefined') {
    window.addEventListener('beforeunload', () => {
      void flush()
    })
  }
}

export function setFrontendTelemetryEnabled(enabled: boolean): void {
  telemetryEnabled = enabled

  if (!enabled) {
    buffer = []
    if (flushTimer) {
      clearTimeout(flushTimer)
      flushTimer = null
    }
  }
}
