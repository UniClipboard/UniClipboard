import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const originalConsole = {
  log: console.log,
  info: console.info,
  warn: console.warn,
  error: console.error,
}

function restoreConsole() {
  console.log = originalConsole.log
  console.info = originalConsole.info
  console.warn = originalConsole.warn
  console.error = originalConsole.error
}

async function loadModule() {
  vi.resetModules()
  return import('../otlp')
}

describe('frontend OTLP logging', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    restoreConsole()
    vi.unstubAllEnvs()
  })

  afterEach(() => {
    restoreConsole()
    vi.useRealTimers()
    vi.restoreAllMocks()
    vi.unstubAllEnvs()
  })

  it('uploads console errors to the OTLP logs endpoint with redaction', async () => {
    vi.stubEnv('VITE_OTEL_EXPORTER_OTLP_ENDPOINT', 'https://seq.example.com/ingest/otlp')
    const fetchMock = vi.fn().mockResolvedValue({ ok: true })
    vi.stubGlobal('fetch', fetchMock)

    const { initFrontendOtlp, setFrontendTelemetryEnabled } = await loadModule()
    initFrontendOtlp()
    setFrontendTelemetryEnabled(true)

    console.error('boom', { password: 'secret', safe: 'value' })
    await vi.advanceTimersByTimeAsync(1_000)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('https://seq.example.com/ingest/otlp/v1/logs')

    const payload = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))
    const record = payload.resourceLogs[0].scopeLogs[0].logRecords[0]
    expect(record.severityText).toBe('ERROR')
    expect(record.body.stringValue).toContain('boom')
    expect(record.body.stringValue).toContain('[REDACTED]')
  })

  it('drops buffered logs immediately when telemetry is disabled', async () => {
    vi.stubEnv('VITE_OTEL_EXPORTER_OTLP_ENDPOINT', 'https://seq.example.com/ingest/otlp')
    const fetchMock = vi.fn().mockResolvedValue({ ok: true })
    vi.stubGlobal('fetch', fetchMock)

    const { initFrontendOtlp, setFrontendTelemetryEnabled } = await loadModule()
    initFrontendOtlp()
    setFrontendTelemetryEnabled(true)

    console.warn('queued-before-disable')
    setFrontendTelemetryEnabled(false)
    await vi.advanceTimersByTimeAsync(1_000)

    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('normalizes a traces endpoint to the OTLP logs path', async () => {
    vi.stubEnv('VITE_OTEL_EXPORTER_OTLP_ENDPOINT', 'https://seq.example.com/ingest/otlp/v1/traces')
    const fetchMock = vi.fn().mockResolvedValue({ ok: true })
    vi.stubGlobal('fetch', fetchMock)

    const { initFrontendOtlp, setFrontendTelemetryEnabled } = await loadModule()
    initFrontendOtlp()
    setFrontendTelemetryEnabled(true)

    console.info('frontend-info')
    await vi.advanceTimersByTimeAsync(1_000)

    expect(fetchMock.mock.calls[0]?.[0]).toBe('https://seq.example.com/ingest/otlp/v1/logs')
  })
})
