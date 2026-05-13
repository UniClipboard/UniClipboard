import type { ErrorEvent } from '@sentry/core'
import * as Sentry from '@sentry/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  DEVICE_ROLE_WEBVIEW,
  applyDeviceMetaToSentry,
  initSentry,
  setFrontendSentryEnabled,
} from '@/observability/sentry'

// Mock Sentry
vi.mock('@sentry/react', async importOriginal => {
  const actual = await importOriginal<typeof Sentry>()
  return {
    ...actual,
    init: vi.fn(),
    browserTracingIntegration: vi.fn(),
    getReplay: vi.fn(),
    replayIntegration: vi.fn(),
    setTag: vi.fn(),
    setUser: vi.fn(),
  }
})

describe('initSentry', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    // 清掉 localStorage 镜像，确保下面 dynamic-import 测试能从"首次启动"状态出发。
    if (typeof window !== 'undefined' && window.localStorage) {
      window.localStorage.removeItem('uc.telemetry_enabled')
    }
    // 重置当前模块的运行时 gate 到 false，避免上条测试的状态泄漏到下一条；
    // 注意这一步会把 localStorage 镜像也写成 'false'，所以放在 removeItem 之后。
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

  it('configures Replay as diagnostics-gated error buffering', () => {
    setFrontendSentryEnabled(true)
    initSentry()

    const initCall = vi.mocked(Sentry.init).mock.calls[0][0]
    expect(initCall.replaysSessionSampleRate).toBe(0)
    expect(initCall.replaysOnErrorSampleRate).toBe(1.0)

    expect(Sentry.replayIntegration).toHaveBeenCalledWith(
      expect.objectContaining({
        beforeErrorSampling: expect.any(Function),
      })
    )

    const replayOptions = vi.mocked(Sentry.replayIntegration).mock.calls[0][0]
    const beforeErrorSampling = replayOptions?.beforeErrorSampling
    expect(beforeErrorSampling?.({} as ErrorEvent)).toBe(true)

    setFrontendSentryEnabled(false)
    expect(beforeErrorSampling?.({} as ErrorEvent)).toBe(false)
  })

  it('stops and resumes Replay with the diagnostics gate', () => {
    vi.clearAllMocks()
    const replay = {
      getRecordingMode: vi.fn(() => undefined),
      startBuffering: vi.fn(),
      stop: vi.fn(() => Promise.resolve()),
    }
    vi.mocked(Sentry.getReplay).mockReturnValue(
      replay as unknown as ReturnType<typeof Sentry.getReplay>
    )

    setFrontendSentryEnabled(false)
    expect(replay.stop).toHaveBeenCalledTimes(1)

    setFrontendSentryEnabled(true)
    expect(replay.startBuffering).toHaveBeenCalledTimes(1)
  })

  it('starts with the runtime gate disabled until settings enable it', async () => {
    initSentry()
    const initCall = vi.mocked(Sentry.init).mock.calls[0][0]
    const beforeSend = initCall.beforeSend!

    const event = { extra: { early: 'startup' } } as unknown as ErrorEvent
    expect(await Promise.resolve(beforeSend(event, {}))).toBeNull()
  })

  it('honors a localStorage mirror of "true" so early startup events are captured', async () => {
    // Simulate a previous session that left the user's preference as enabled.
    window.localStorage.setItem('uc.telemetry_enabled', 'true')
    vi.resetModules()
    const fresh = await import('@/observability/sentry')
    fresh.initSentry()
    const calls = vi.mocked(Sentry.init).mock.calls
    const initCall = calls[calls.length - 1][0]
    const beforeSend = initCall.beforeSend!

    const event = { extra: { early: 'startup' } } as unknown as ErrorEvent
    const result = await Promise.resolve(beforeSend(event, {}))
    expect(result).not.toBeNull()
    expect(result?.extra).toEqual({ early: 'startup' })
  })

  it('persists the runtime gate to localStorage so the next session can read it synchronously', () => {
    setFrontendSentryEnabled(true)
    expect(window.localStorage.getItem('uc.telemetry_enabled')).toBe('true')

    setFrontendSentryEnabled(false)
    expect(window.localStorage.getItem('uc.telemetry_enabled')).toBe('false')
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

  it('applies camelCase device meta to Sentry scope', () => {
    applyDeviceMetaToSentry({
      deviceId: 'device-a',
      deviceRole: 'gui-host',
      platform: 'macos',
      appVersion: '1.2.3',
      appChannel: 'dev',
    })

    expect(Sentry.setUser).toHaveBeenCalledWith({ id: 'device-a' })
    expect(Sentry.setTag).toHaveBeenCalledWith('device.id', 'device-a')
    expect(Sentry.setTag).toHaveBeenCalledWith('device.role', DEVICE_ROLE_WEBVIEW)
    expect(Sentry.setTag).toHaveBeenCalledWith('device.host_role', 'gui-host')
    expect(Sentry.setTag).toHaveBeenCalledWith('app.version', '1.2.3')
    expect(Sentry.setTag).toHaveBeenCalledWith('app.channel', 'dev')
  })
})
