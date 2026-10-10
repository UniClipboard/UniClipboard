import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/lib/platform', () => ({
  applyPlatformEffectPreferences: vi.fn(),
  detectPlatformInfo: vi.fn(() => ({ isWindows: false })),
}))
vi.mock('@/lib/ui-scale', () => ({ initializeUiScale: vi.fn(() => vi.fn()) }))
vi.mock('@/lib/ui-sound', () => ({ initializeUiSound: vi.fn(() => vi.fn()) }))

import { initializeWindowUi } from '@/lib/window-ui'

describe('window scrollbar visibility', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
    document.body.replaceChildren()
  })

  it('shows a native scrollbar while scrolling and hides it after scrolling stops', () => {
    const dispose = initializeWindowUi()
    const scrollable = document.createElement('div')
    document.body.append(scrollable)

    scrollable.dispatchEvent(new Event('scroll'))
    expect(scrollable).toHaveAttribute('data-scroll-active', 'true')

    vi.advanceTimersByTime(499)
    expect(scrollable).toHaveAttribute('data-scroll-active', 'true')

    vi.advanceTimersByTime(1)
    expect(scrollable).not.toHaveAttribute('data-scroll-active')

    dispose()
  })
})
