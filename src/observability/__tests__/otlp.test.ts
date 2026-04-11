import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { invokeWithTrace } from '@/lib/tauri-command'

vi.mock('@/lib/tauri-command', () => ({
  invokeWithTrace: vi.fn(),
}))

async function loadModule() {
  vi.resetModules()
  return import('../otlp')
}

describe('frontend OTLP logging', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.unstubAllEnvs()
    Reflect.deleteProperty(window, '__TAURI__')
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
    vi.unstubAllEnvs()
    Reflect.deleteProperty(window, '__TAURI__')
  })

  it('flushes queued records to the OTLP logs endpoint', async () => {
    vi.stubEnv('VITE_OTEL_EXPORTER_OTLP_ENDPOINT', 'https://seq.example.com/ingest/otlp')
    const fetchMock = vi.fn().mockResolvedValue({ ok: true })
    vi.stubGlobal('fetch', fetchMock)

    const { initFrontendOtlp, setFrontendTelemetryEnabled, queueLogRecord } = await loadModule()
    initFrontendOtlp()
    setFrontendTelemetryEnabled(true)

    queueLogRecord({
      timeUnixNano: '1700000000000000000',
      severityNumber: 17,
      severityText: 'ERROR',
      body: { stringValue: 'boom [REDACTED]' },
    })
    await vi.advanceTimersByTimeAsync(1_000)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('https://seq.example.com/ingest/otlp/v1/logs')

    const payload = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))
    const record = payload.resourceLogs[0].scopeLogs[0].logRecords[0]
    expect(record.severityText).toBe('ERROR')
    expect(record.body.stringValue).toContain('boom')
    expect(record.body.stringValue).toContain('[REDACTED]')
  })

  it('drops queued records immediately when telemetry is disabled', async () => {
    vi.stubEnv('VITE_OTEL_EXPORTER_OTLP_ENDPOINT', 'https://seq.example.com/ingest/otlp')
    const fetchMock = vi.fn().mockResolvedValue({ ok: true })
    vi.stubGlobal('fetch', fetchMock)

    const { initFrontendOtlp, setFrontendTelemetryEnabled, queueLogRecord } = await loadModule()
    initFrontendOtlp()
    setFrontendTelemetryEnabled(true)

    queueLogRecord({
      timeUnixNano: '1700000000000000000',
      severityNumber: 13,
      severityText: 'WARN',
      body: { stringValue: 'queued-before-disable' },
    })
    setFrontendTelemetryEnabled(false)
    await vi.advanceTimersByTimeAsync(1_000)

    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('normalizes a traces endpoint to the OTLP logs path', async () => {
    vi.stubEnv('VITE_OTEL_EXPORTER_OTLP_ENDPOINT', 'https://seq.example.com/ingest/otlp/v1/traces')
    const fetchMock = vi.fn().mockResolvedValue({ ok: true })
    vi.stubGlobal('fetch', fetchMock)

    const { initFrontendOtlp, setFrontendTelemetryEnabled, queueLogRecord } = await loadModule()
    initFrontendOtlp()
    setFrontendTelemetryEnabled(true)

    queueLogRecord({
      timeUnixNano: '1700000000000000000',
      severityNumber: 9,
      severityText: 'INFO',
      body: { stringValue: 'frontend-info' },
    })
    await vi.advanceTimersByTimeAsync(1_000)

    expect(fetchMock.mock.calls[0]?.[0]).toBe('https://seq.example.com/ingest/otlp/v1/logs')
  })

  it('attaches the runtime device id to frontend OTLP resource attributes', async () => {
    vi.stubEnv('VITE_OTEL_EXPORTER_OTLP_ENDPOINT', 'https://seq.example.com/ingest/otlp')
    const fetchMock = vi.fn().mockResolvedValue({ ok: true })
    vi.stubGlobal('fetch', fetchMock)
    Reflect.set(window, '__TAURI__', {})
    vi.mocked(invokeWithTrace).mockResolvedValueOnce('device-telemetry-123')

    const { initFrontendOtlp, setFrontendTelemetryEnabled, queueLogRecord } = await loadModule()
    initFrontendOtlp()
    setFrontendTelemetryEnabled(true)

    queueLogRecord({
      timeUnixNano: '1700000000000000000',
      severityNumber: 9,
      severityText: 'INFO',
      body: { stringValue: 'frontend-info' },
    })
    await vi.advanceTimersByTimeAsync(1_000)

    expect(invokeWithTrace).toHaveBeenCalledWith('get_device_id')

    const payload = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))
    const resourceAttributes = payload.resourceLogs[0].resource.attributes

    expect(resourceAttributes).toEqual(
      expect.arrayContaining([
        { key: 'device_id', value: { stringValue: 'device-telemetry-123' } },
        { key: 'service.instance.id', value: { stringValue: 'device-telemetry-123' } },
      ])
    )
  })

  it('silently drops records when no endpoint is configured', async () => {
    vi.unstubAllEnvs()
    const fetchMock = vi.fn().mockResolvedValue({ ok: true })
    vi.stubGlobal('fetch', fetchMock)

    const { initFrontendOtlp, setFrontendTelemetryEnabled, queueLogRecord } = await loadModule()
    initFrontendOtlp()
    setFrontendTelemetryEnabled(true)

    queueLogRecord({
      timeUnixNano: '1700000000000000000',
      severityNumber: 9,
      severityText: 'INFO',
      body: { stringValue: 'no-endpoint' },
    })
    await vi.advanceTimersByTimeAsync(1_000)

    expect(fetchMock).not.toHaveBeenCalled()
  })
})
