import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Mock Sentry's namespaced logger so we can assert which severity the pino
// transmit hook routes each pino level to and what attributes it attaches.
const sentryLogger = {
  trace: vi.fn(),
  debug: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  fatal: vi.fn(),
  fmt: (strings: TemplateStringsArray, ...values: unknown[]) =>
    strings.reduce((acc, part, i) => acc + part + (i < values.length ? String(values[i]) : ''), ''),
}

vi.mock('@sentry/react', () => ({
  logger: sentryLogger,
}))

// traceManager is consulted on every transmit; mock so individual cases can
// inject a known traceId without spinning up the real Sentry pipeline.
const getCurrentTrace = vi.fn<() => { traceId: string } | null>(() => null)
vi.mock('@/observability/trace', () => ({
  traceManager: {
    getCurrentTrace,
  },
}))

async function loadLogger() {
  vi.resetModules()
  return import('@/lib/logger')
}

describe('frontend pino → Sentry Logs bridge', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    getCurrentTrace.mockReturnValue(null)
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('routes pino.info() to Sentry.logger.info with the module attribute', async () => {
    const { createLogger } = await loadLogger()
    const log = createLogger('daemon-ws')

    log.info('connected')

    expect(sentryLogger.info).toHaveBeenCalledTimes(1)
    const [message, attributes] = sentryLogger.info.mock.calls[0]
    expect(message).toBe('connected')
    expect(attributes).toEqual({ module: 'daemon-ws' })
  })

  it('routes pino.error() to Sentry.logger.error and surfaces the message text', async () => {
    const { createLogger } = await loadLogger()
    const log = createLogger('worker')

    log.error('task failed')

    expect(sentryLogger.error).toHaveBeenCalledTimes(1)
    const [message, attributes] = sentryLogger.error.mock.calls[0]
    expect(message).toBe('task failed')
    expect(attributes).toEqual({ module: 'worker' })
  })

  it('redacts sensitive fields embedded in object arguments', async () => {
    const { logger } = await loadLogger()

    logger.warn({ password: 'hunter2', other: 'ok' }, 'auth event')

    expect(sentryLogger.warn).toHaveBeenCalledTimes(1)
    const [message] = sentryLogger.warn.mock.calls[0]
    // Pino concatenates child bindings + message args — the redacted object
    // becomes part of the rendered message string.
    expect(message).toContain('[REDACTED]')
    expect(message).toContain('auth event')
    expect(message).not.toContain('hunter2')
  })

  it('attaches the active trace_id from traceManager when present', async () => {
    getCurrentTrace.mockReturnValue({ traceId: 'trace-abc' })
    const { createLogger } = await loadLogger()
    const log = createLogger('clipboard')

    log.info('captured')

    expect(sentryLogger.info).toHaveBeenCalledTimes(1)
    const [, attributes] = sentryLogger.info.mock.calls[0]
    expect(attributes).toEqual({ module: 'clipboard', trace_id: 'trace-abc' })
  })

  it('omits the attributes argument when neither module nor trace_id apply', async () => {
    const { logger } = await loadLogger()

    logger.info('plain message')

    expect(sentryLogger.info).toHaveBeenCalledTimes(1)
    const [, attributes] = sentryLogger.info.mock.calls[0]
    expect(attributes).toBeUndefined()
  })

  it('does not transmit pino.debug() because the transmit threshold is info', async () => {
    const { createLogger } = await loadLogger()
    const log = createLogger('verbose')

    log.debug('chatty')

    expect(sentryLogger.debug).not.toHaveBeenCalled()
    expect(sentryLogger.info).not.toHaveBeenCalled()
  })
})
