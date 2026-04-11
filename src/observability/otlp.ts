import { getDeviceId } from '@/api/runtime'
import { redactSensitiveArgs } from '@/observability/redaction'

type OtlpAnyValue =
  | { stringValue: string }
  | { boolValue: boolean }
  | { intValue: number }
  | { doubleValue: number }

type OtlpKeyValue = {
  key: string
  value: OtlpAnyValue
}

export type OtlpLogRecord = {
  timeUnixNano: string
  severityNumber: number
  severityText: string
  body: { stringValue: string }
  attributes?: OtlpKeyValue[]
}

const FLUSH_INTERVAL_MS = 1_000
const SERVICE_NAME = 'uniclipboard-frontend'
const SCOPE_NAME = 'uniclipboard.frontend.console'

let buffer: OtlpLogRecord[] = []
let flushTimer: ReturnType<typeof setTimeout> | null = null
let logsEndpoint = ''
let initialized = false
let telemetryEnabled = false
let frontendDeviceId: string | null = null
let frontendDeviceIdPromise: Promise<string | null> | null = null

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
    console.warn(
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

  if (frontendDeviceId) {
    attrs.push({ key: 'device_id', value: { stringValue: frontendDeviceId } })
    attrs.push({ key: 'service.instance.id', value: { stringValue: frontendDeviceId } })
  }

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

function resolveFrontendDeviceId(): Promise<string | null> {
  if (frontendDeviceId) {
    return Promise.resolve(frontendDeviceId)
  }

  if (frontendDeviceIdPromise) {
    return frontendDeviceIdPromise
  }

  if (typeof window === 'undefined' || !('__TAURI__' in window)) {
    return Promise.resolve(null)
  }

  frontendDeviceIdPromise = getDeviceId()
    .then(deviceId => {
      const trimmed = deviceId.trim()
      frontendDeviceId = trimmed || null
      return frontendDeviceId
    })
    .catch(error => {
      const safeError =
        error instanceof Error
          ? `${error.name}: ${error.message}`
          : String(redactSensitiveArgs(error))
      console.warn('[OTLP] failed to resolve device_id:', safeError)
      return null
    })
    .finally(() => {
      frontendDeviceIdPromise = null
    })

  return frontendDeviceIdPromise
}

async function flush(): Promise<void> {
  if (!telemetryEnabled || !logsEndpoint || buffer.length === 0) {
    buffer = []
    flushTimer = null
    return
  }

  await resolveFrontendDeviceId()
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
    const safeError =
      error instanceof Error
        ? `${error.name}: ${error.message}`
        : String(redactSensitiveArgs(error))
    console.warn('[OTLP] flush failed:', safeError)
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

/**
 * Enqueue a structured OTLP log record for batched export.
 * Called by the pino transmit handler in src/lib/logger.ts.
 */
export function queueLogRecord(record: OtlpLogRecord): void {
  if (!telemetryEnabled || !logsEndpoint) {
    return
  }

  buffer.push(record)
  scheduleFlush()
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

  void resolveFrontendDeviceId()

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
