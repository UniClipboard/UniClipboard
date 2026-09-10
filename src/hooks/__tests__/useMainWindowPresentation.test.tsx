import { renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { useMainWindowPresentation } from '@/hooks/useMainWindowPresentation'

const report = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
vi.mock('@/api/main-window', () => ({ reportMainWindowPresentationReady: report }))

describe('main window presentation', () => {
  it('reports a committed destination once, not the transient startup screen', () => {
    const { rerender } = renderHook(({ ready }) => useMainWindowPresentation(ready), {
      initialProps: { ready: false },
    })
    expect(report).not.toHaveBeenCalled()
    rerender({ ready: true })
    expect(report).toHaveBeenCalledOnce()
    rerender({ ready: true })
    expect(report).toHaveBeenCalledOnce()
  })
})
