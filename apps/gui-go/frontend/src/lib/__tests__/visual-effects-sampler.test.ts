import { afterEach, describe, expect, it, vi } from 'vitest'
const mock = vi.hoisted(() => ({
  begin: vi.fn(),
  report: vi.fn(),
  state: { sessionId: 'gui', revision: 1, mode: 'auto', reduceMotion: false },
}))
vi.mock('@/api/visual-effects', () => ({
  visualEffectsApi: { beginSample: mock.begin, reportSample: mock.report },
}))
vi.mock('@/lib/visual-effects-store', () => ({
  visualEffectsStore: { getSnapshot: () => mock.state, subscribe: () => () => {}, accept: vi.fn() },
}))
import { startVisualEffectsSampler, summarizeFrames } from '@/lib/visual-effects-sampler'

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  mock.begin.mockReset()
  mock.report.mockReset()
})

describe('visual effects samples', () => {
  it.each([30, 60, 120])('does not mistake a normal %i Hz display for severe stalls', hz => {
    const result = summarizeFrames(Array.from({ length: hz }, () => 1000 / hz))
    expect(result?.longFrames).toBe(0)
  })
  it('preserves a single stall as one, and rejects resumed or invalid intervals', () => {
    const normal = Array.from({ length: 60 }, () => 16)
    expect(summarizeFrames([...normal, 100])?.longFrames).toBe(1)
    expect(summarizeFrames([...normal, 501])).toBeNull()
    expect(summarizeFrames([...normal, NaN])).toBeNull()
    expect(summarizeFrames([16])).toBeNull()
    expect(summarizeFrames(Array.from({ length: 100 }, () => 50))).toBeNull()
  })
  it('does not sample synthetic events or hammer IPC after a refused permit', async () => {
    let now = 0
    vi.spyOn(performance, 'now').mockImplementation(() => now)
    const add = vi.spyOn(document, 'addEventListener')
    const frame = vi.fn()
    vi.stubGlobal('requestAnimationFrame', frame)
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
    mock.begin.mockResolvedValue(null)
    const stop = startVisualEffectsSampler(() => true)
    const interaction = add.mock.calls.find(([name]) => name === 'pointerdown')?.[1] as (
      event: Event
    ) => void
    now = 11000
    interaction({ isTrusted: false } as Event)
    expect(mock.begin).not.toHaveBeenCalled()
    interaction({ isTrusted: true } as Event)
    await Promise.resolve()
    interaction({ isTrusted: true } as Event)
    expect(mock.begin).toHaveBeenCalledTimes(1)
    expect(frame).not.toHaveBeenCalled()
    stop()
  })
  it('cancels the pending frame on visibility changes and disposal', async () => {
    let now = 0
    vi.spyOn(performance, 'now').mockImplementation(() => now)
    const add = vi.spyOn(document, 'addEventListener')
    const cancel = vi.fn()
    vi.stubGlobal(
      'requestAnimationFrame',
      vi.fn(() => 9)
    )
    vi.stubGlobal('cancelAnimationFrame', cancel)
    mock.begin.mockResolvedValue({ sessionId: 'gui', revision: 1, sampleId: 1 })
    const stop = startVisualEffectsSampler(() => true)
    const interaction = add.mock.calls.find(([name]) => name === 'pointerdown')?.[1] as (
      event: Event
    ) => void
    now = 11000
    interaction({ isTrusted: true } as Event)
    await Promise.resolve()
    document.dispatchEvent(new Event('visibilitychange'))
    expect(cancel).toHaveBeenCalledWith(9)
    expect(mock.report).not.toHaveBeenCalled()
    stop()
  })
})
