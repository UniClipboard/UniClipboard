import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { readListWidth, writeListWidth } from '../history-layout'
import { useHistoryListColumn } from '../useHistoryListColumn'

const panel = vi.hoisted(() => ({
  size: 560,
  resize: vi.fn(),
  getSize: vi.fn(),
}))

vi.mock('react-resizable-panels', () => ({
  usePanelRef: () => ({ current: panel }),
}))

function setWindowWidth(width: number) {
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: width })
  window.dispatchEvent(new Event('resize'))
}

describe('useHistoryListColumn', () => {
  beforeEach(() => {
    panel.size = 560
    panel.resize.mockClear()
    panel.getSize.mockImplementation(() => ({ inPixels: panel.size, asPercentage: 50 }))
    setWindowWidth(1280)
  })
  afterEach(() => localStorage.clear())

  it('applies the remembered width on mount and the tier default when crossing tiers', () => {
    writeListWidth('standard', 500)
    const { result } = renderHook(() => useHistoryListColumn())

    expect(result.current.tier).toBe('standard')
    expect(result.current.panelProps).toMatchObject({
      defaultSize: '560px',
      minSize: '360px',
      maxSize: '560px',
    })
    expect(panel.resize).toHaveBeenLastCalledWith('500px')

    act(() => setWindowWidth(900))
    expect(result.current.tier).toBe('compact')
    expect(panel.resize).toHaveBeenLastCalledWith('400px')
  })

  it('returns to the intended width when the group grows back within a tier', () => {
    let notify = () => {}
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: () => void) {
          notify = callback
        }
        observe() {}
        disconnect() {}
      }
    )
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => {
      callback(0)
      return 0
    })
    const { result } = renderHook(() => useHistoryListColumn())
    act(() => result.current.groupProps.elementRef(document.createElement('div')))
    panel.resize.mockClear()

    // The window shrank and the detail floor clamped the list; it grows back.
    panel.size = 459
    act(() => notify())
    expect(panel.resize).toHaveBeenLastCalledWith('560px')

    // Already at the intended width: nothing to do.
    panel.resize.mockClear()
    panel.size = 560
    act(() => notify())
    expect(panel.resize).not.toHaveBeenCalled()
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('remembers a dragged width but not a clamp caused by resizing the window', () => {
    const { result } = renderHook(() => useHistoryListColumn())

    panel.size = 420
    act(() => result.current.groupProps.onLayoutChanged())
    expect(readListWidth('standard')).toBeNull()

    act(() => result.current.handleProps.onPointerDown())
    panel.size = 470
    act(() => result.current.groupProps.onLayoutChanged())
    expect(readListWidth('standard')).toBe(470)
  })
})
