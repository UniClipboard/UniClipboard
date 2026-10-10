import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import type { AppContentView } from '@/components/app/app-content-state'
import { POST_UNLOCK_HOLD_MS, useSettledAppView } from '@/hooks/useSettledAppView'

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

const setup = (initial: AppContentView) =>
  renderHook(({ view }) => useSettledAppView(view), { initialProps: { view: initial } })

it('keeps the unlock page through a brief startup gap and goes straight to the app', () => {
  const { result, rerender } = setup('unlock')
  rerender({ view: 'startup' })
  expect(result.current).toBe('unlock')

  act(() => void vi.advanceTimersByTime(POST_UNLOCK_HOLD_MS - 1))
  rerender({ view: 'authenticated' })
  expect(result.current).toBe('authenticated')
})

it('shows the startup page once the post-unlock check takes too long', () => {
  const { result, rerender } = setup('unlock')
  rerender({ view: 'startup' })
  act(() => void vi.advanceTimersByTime(POST_UNLOCK_HOLD_MS))
  expect(result.current).toBe('startup')
})

it('keeps the app through a brief startup gap after unlocking', () => {
  const { result, rerender } = setup('unlock')
  rerender({ view: 'authenticated' })
  expect(result.current).toBe('authenticated')

  rerender({ view: 'startup' })
  expect(result.current).toBe('authenticated')
  act(() => void vi.advanceTimersByTime(POST_UNLOCK_HOLD_MS - 1))
  rerender({ view: 'authenticated' })
  expect(result.current).toBe('authenticated')
})

it('also holds a brief upgrade view that follows the app view', () => {
  const { result, rerender } = setup('authenticated')
  rerender({ view: 'upgrade' })
  expect(result.current).toBe('authenticated')
  rerender({ view: 'authenticated' })
  expect(result.current).toBe('authenticated')
})

it('does not delay any other transition', () => {
  const { result, rerender } = setup('startup')
  rerender({ view: 'unlock' })
  expect(result.current).toBe('unlock')
  rerender({ view: 'failure' })
  expect(result.current).toBe('failure')
  rerender({ view: 'upgrade' })
  expect(result.current).toBe('upgrade')
})
