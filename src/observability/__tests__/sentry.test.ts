import type { ErrorEvent } from '@sentry/core'
import * as Sentry from '@sentry/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { initSentry, setFrontendSentryEnabled } from '@/observability/sentry'

// Mock Sentry
vi.mock('@sentry/react', async importOriginal => {
  const actual = await importOriginal<typeof Sentry>()
  return {
    ...actual,
    init: vi.fn(),
    browserTracingIntegration: vi.fn(),
    replayIntegration: vi.fn(),
  }
})

describe('initSentry', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    // 重置到启动默认值，避免设置加载前误发。
    setFrontendSentryEnabled(false)
  })

  it('initializes Sentry with correct configuration', () => {
    initSentry()

    expect(Sentry.init).toHaveBeenCalledWith(
      expect.objectContaining({
        dsn: import.meta.env.VITE_SENTRY_DSN,
        environment: import.meta.env.MODE,
        release: import.meta.env.VITE_APP_VERSION,
        beforeSend: expect.any(Function),
        beforeBreadcrumb: expect.any(Function),
      })
    )
  })

  it('scrubs sensitive data from breadcrumbs', () => {
    setFrontendSentryEnabled(true)
    initSentry()
    const initCall = vi.mocked(Sentry.init).mock.calls[0][0]
    const beforeBreadcrumb = initCall.beforeBreadcrumb!

    const breadcrumb = {
      data: {
        password: 'secret',
        other: 'value',
      },
      message: 'test',
    }

    const result = beforeBreadcrumb(breadcrumb, {})

    expect(result?.data).toEqual({
      password: '[REDACTED]',
      other: 'value',
    })
  })

  it('scrubs sensitive data from event extra', async () => {
    setFrontendSentryEnabled(true)
    initSentry()
    const initCall = vi.mocked(Sentry.init).mock.calls[0][0]
    const beforeSend = initCall.beforeSend!

    const event = {
      extra: {
        apiKey: '12345',
        safe: 'data',
      },
    } as unknown as ErrorEvent

    const result = await Promise.resolve(beforeSend(event, {}))

    expect(result?.extra).toEqual({
      apiKey: '[REDACTED]',
      safe: 'data',
    })
  })

  it('preserves existing ResizeObserver filter in beforeSend', async () => {
    setFrontendSentryEnabled(true)
    initSentry()
    const initCall = vi.mocked(Sentry.init).mock.calls[0][0]
    const beforeSend = initCall.beforeSend!

    const resizeEvent = {
      exception: {
        values: [{ type: 'ResizeObserver loop limit exceeded' }],
      },
    } as unknown as ErrorEvent

    const result = await Promise.resolve(beforeSend(resizeEvent, {}))

    expect(result).toBeNull()
  })

  it('drops events when telemetry runtime gate is disabled', async () => {
    initSentry()
    const initCall = vi.mocked(Sentry.init).mock.calls[0][0]
    const beforeSend = initCall.beforeSend!
    const beforeBreadcrumb = initCall.beforeBreadcrumb!
    const beforeSendLog = initCall.beforeSendLog!

    setFrontendSentryEnabled(false)

    const event = { extra: { foo: 'bar' } } as unknown as ErrorEvent
    const breadcrumb: Sentry.Breadcrumb = { message: 'click' }
    const log = { body: 'hello', attributes: { x: 1 } } as unknown as Parameters<
      NonNullable<typeof beforeSendLog>
    >[0]

    expect(await Promise.resolve(beforeSend(event, {}))).toBeNull()
    expect(beforeBreadcrumb(breadcrumb, {})).toBeNull()
    expect(beforeSendLog(log)).toBeNull()
  })

  it('starts with the runtime gate disabled until settings enable it', async () => {
    initSentry()
    const initCall = vi.mocked(Sentry.init).mock.calls[0][0]
    const beforeSend = initCall.beforeSend!

    const event = { extra: { early: 'startup' } } as unknown as ErrorEvent
    expect(await Promise.resolve(beforeSend(event, {}))).toBeNull()
  })

  it('passes events through when telemetry runtime gate is re-enabled', async () => {
    initSentry()
    const initCall = vi.mocked(Sentry.init).mock.calls[0][0]
    const beforeSend = initCall.beforeSend!

    setFrontendSentryEnabled(false)
    setFrontendSentryEnabled(true)

    const event = { extra: { safe: 'data' } } as unknown as ErrorEvent
    const result = await Promise.resolve(beforeSend(event, {}))

    expect(result).not.toBeNull()
    expect(result?.extra).toEqual({ safe: 'data' })
  })
})
