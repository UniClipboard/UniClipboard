import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const logError = vi.fn()
vi.mock('@/lib/logger', () => ({
  createLogger: () => ({ error: logError }),
}))

async function load() {
  vi.resetModules()
  return import('@/observability/global-errors')
}

function rejection(reason: unknown): Event {
  const event = new Event('unhandledrejection') as Event & { reason?: unknown }
  event.reason = reason
  return event
}

describe('installGlobalErrorHandlers', () => {
  let target: EventTarget

  beforeEach(() => {
    logError.mockClear()
    target = new EventTarget()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('logs an Error rejection reason with name, message and stack', async () => {
    const { installGlobalErrorHandlers } = await load()
    installGlobalErrorHandlers(target)

    target.dispatchEvent(rejection(new RangeError('bad range')))

    expect(logError).toHaveBeenCalledTimes(1)
    const [fields, message] = logError.mock.calls[0]
    expect(message).toBe('Unhandled promise rejection')
    expect(fields.reason).toMatchObject({ name: 'RangeError', message: 'bad range' })
    expect(fields.reason.stack).toEqual(expect.any(String))
  })

  it('logs string and object rejection reasons', async () => {
    const { installGlobalErrorHandlers } = await load()
    installGlobalErrorHandlers(target)

    target.dispatchEvent(rejection('just a string'))
    target.dispatchEvent(rejection({ code: 'E_X', detail: 'd' }))

    expect(logError.mock.calls[0][0].reason).toBe('just a string')
    expect(logError.mock.calls[1][0].reason).toEqual({ code: 'E_X', detail: 'd' })
  })

  it('logs window error events with location', async () => {
    const { installGlobalErrorHandlers } = await load()
    installGlobalErrorHandlers(target)
    const event = new Event('error') as Event & Record<string, unknown>
    Object.assign(event, {
      message: 'Script error',
      filename: 'app.js',
      lineno: 3,
      colno: 9,
      error: new Error('thrown'),
    })

    target.dispatchEvent(event)

    const [fields, message] = logError.mock.calls[0]
    expect(message).toBe('Uncaught error')
    expect(fields).toMatchObject({ filename: 'app.js', lineno: 3, colno: 9 })
    expect(fields.error).toMatchObject({ message: 'thrown' })
  })

  it('installs exactly once per target', async () => {
    const { installGlobalErrorHandlers } = await load()
    installGlobalErrorHandlers(target)
    installGlobalErrorHandlers(target)

    target.dispatchEvent(rejection('once'))

    expect(logError).toHaveBeenCalledTimes(1)
  })
})
