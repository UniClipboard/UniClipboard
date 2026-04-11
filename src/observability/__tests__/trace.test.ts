import { describe, expect, it, vi } from 'vitest'
vi.mock('@/observability/sentry', () => ({
  Sentry: {
    startInactiveSpan: vi.fn(() => ({
      spanContext: () => ({ traceId: crypto.randomUUID() }),
      end: vi.fn(),
    })),
  },
  sentryEnabled: true,
}))
import { traceManager } from '../trace'

describe('traceManager', () => {
  it('generates unique trace ids', () => {
    const first = traceManager.startTrace('test')
    traceManager.endTrace()
    const second = traceManager.startTrace('test')
    expect(first.traceId).not.toBe(second.traceId)
    traceManager.endTrace()
  })

  it('creates a Sentry span on startTrace', async () => {
    const { Sentry } = await import('@/observability/sentry')
    const trace = traceManager.startTrace('test.operation')
    expect(Sentry.startInactiveSpan).toHaveBeenCalledWith({
      name: 'test.operation',
      op: 'ui.action',
    })
    expect(trace.sentrySpan).toBeDefined()
    traceManager.endTrace()
  })

  it('ends the Sentry span on endTrace', async () => {
    const trace = traceManager.startTrace('test.end')
    const endSpy = trace.sentrySpan!.end as ReturnType<typeof vi.fn>
    traceManager.endTrace()
    expect(endSpy).toHaveBeenCalled()
    expect(traceManager.getCurrentTrace()).toBeNull()
  })
})
