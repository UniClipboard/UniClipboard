import { describe, expect, it, vi } from 'vitest'
vi.mock('@/api/visual-effects', () => ({ visualEffectsApi: {} }))
vi.mock('@/lib/visual-effects-motion', () => ({ applyMotionPreference: vi.fn() }))
import type { visualEffectsApi } from '@/api/visual-effects'
import { createVisualEffectsStore, INITIAL_EFFECTS } from '@/lib/visual-effects-store'

describe('visual effects snapshots', () => {
  it('keeps the latest revision when a delayed read returns after an event', async () => {
    let resolve!: (value: typeof INITIAL_EFFECTS) => void
    const api = {
      get: () =>
        new Promise(r => {
          resolve = r
        }),
    } as unknown as typeof visualEffectsApi
    const apply = vi.fn()
    const store = createVisualEffectsStore(api, apply)
    const read = store.refresh()
    store.accept({
      ...INITIAL_EFFECTS,
      sessionId: 'gui',
      revision: 4,
      mode: 'effects',
      reduceMotion: false,
    })
    resolve({ ...INITIAL_EFFECTS, sessionId: 'gui', revision: 2 })
    await read
    expect(store.getSnapshot().revision).toBe(4)
    expect(store.getSnapshot().mode).toBe('effects')
    expect(apply).toHaveBeenCalledTimes(1)
  })
  it('applies session-only responses but exposes command failures without changing selection', async () => {
    const api = {
      setMode: vi
        .fn()
        .mockResolvedValue({ ...INITIAL_EFFECTS, sessionId: 'gui', revision: 1, mode: 'effects' }),
    } as unknown as typeof visualEffectsApi
    const store = createVisualEffectsStore(api, vi.fn())
    await store.setMode('effects')
    expect(store.getSnapshot().persistence).toBe('session_only')
    vi.mocked(api.setMode).mockRejectedValue(new Error('offline'))
    await expect(store.setMode('smooth')).rejects.toThrow()
    expect(store.getSnapshot().mode).toBe('effects')
    expect(store.isUnavailable()).toBe(true)
  })
  it('ignores duplicate snapshots and old GUI sessions', () => {
    const store = createVisualEffectsStore({} as typeof visualEffectsApi, vi.fn())
    const listener = vi.fn()
    store.subscribe(listener)
    const value = { ...INITIAL_EFFECTS, sessionId: 'gui', revision: 1 }
    store.accept(value)
    store.accept(value)
    store.accept({ ...value, sessionId: 'old', revision: 100 })
    expect(listener).toHaveBeenCalledTimes(1)
  })
})
