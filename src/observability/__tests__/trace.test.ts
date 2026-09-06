import * as Sentry from '@sentry/react'
import { describe, expect, it, vi } from 'vitest'
import { traceManager } from '../trace'

vi.mock('@sentry/react', async importOriginal => ({
  ...(await importOriginal<typeof Sentry>()),
  startInactiveSpan: vi.fn(() => ({
    spanContext: () => ({ traceId: crypto.randomUUID() }),
    end: vi.fn(),
  })),
}))

describe('diagnostic operation ownership', () => {
  it('does not expose provider objects to callers', () => {
    const trace = traceManager.startTrace('copy')
    expect(trace).not.toHaveProperty('sentrySpan')
    traceManager.endTrace(trace)
  })

  it('finishes only the requested operation when calls overlap', () => {
    const first = traceManager.startTrace('first')
    const firstSpan = vi.mocked(Sentry.startInactiveSpan).mock.results.slice(-1)[0]!.value
    const second = traceManager.startTrace('second')
    const secondSpan = vi.mocked(Sentry.startInactiveSpan).mock.results.slice(-1)[0]!.value

    expect(traceManager.getCurrentTrace()).toBeNull()
    traceManager.endTrace(first)
    expect(firstSpan.end).toHaveBeenCalledOnce()
    expect(secondSpan.end).not.toHaveBeenCalled()
    expect(traceManager.getCurrentTrace()).toBe(second)
    traceManager.endTrace(first)
    expect(firstSpan.end).toHaveBeenCalledOnce()
    traceManager.endTrace(second)
    expect(secondSpan.end).toHaveBeenCalledOnce()
    expect(traceManager.getCurrentTrace()).toBeNull()
  })
})
