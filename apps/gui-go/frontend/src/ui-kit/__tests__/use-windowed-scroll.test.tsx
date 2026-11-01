import { act, fireEvent, render } from '@testing-library/react'
import { useRef } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { HistoryWindowViewport } from '@/ui-kit/contract'
import { moveWindowSelection, useWindowedScroll } from '@/ui-kit/use-windowed-scroll'
import { absoluteVisibleRange, windowedGeometry } from '@/ui-kit/windowed-geometry'

function rect(top: number, height: number) {
  return {
    top,
    bottom: top + height,
    height,
    left: 0,
    right: 400,
    width: 400,
    x: 0,
    y: top,
    toJSON: () => ({}),
  } as DOMRect
}

describe('windowed geometry', () => {
  it('keeps a global list extent across eviction, idle shrink and pending rows', () => {
    expect(windowedGeometry(30, 50, 200, 20)).toMatchObject({ top: 600, bottom: 2400 })
    expect(windowedGeometry(30, 25, 200, 20)).toMatchObject({ top: 600, bottom: 2900 })
    expect(windowedGeometry(0, 50, 203, 20)).toMatchObject({ top: 0, bottom: 3060 })
    expect(windowedGeometry(0, 3, 2, 20).bottom).toBe(0)
    expect(windowedGeometry(-10, -5, -2, -20)).toMatchObject({ top: 0, bottom: 0 })
  })

  it('uses absolute grid rows and aligns partial rows by column count', () => {
    expect(windowedGeometry(7, 8, 23, 100, 3)).toEqual({
      top: 200,
      bottom: 300,
      topRows: 2,
      bottomRows: 3,
      leadingColumns: 1,
    })
    expect(windowedGeometry(8, 8, 23, 100, 4)).toMatchObject({
      top: 200,
      bottom: 200,
      leadingColumns: 0,
    })
    expect(windowedGeometry(21, 3, 23, 100, 3).bottom).toBe(0)
  })

  it('maps scroll positions in unloaded space to absolute list and grid indices', () => {
    expect(absoluteVisibleRange(2400, 100, 20, 200)).toEqual({ start: 120, end: 124 })
    expect(absoluteVisibleRange(700, 200, 100, 23, 3)).toEqual({ start: 21, end: 22 })
    expect(absoluteVisibleRange(-50, 100, 20, 0)).toEqual({ start: 0, end: 0 })
  })
})

describe('useWindowedScroll', () => {
  let frames: Map<number, FrameRequestCallback>
  let nextFrame: number
  const onVisibleRange = vi.fn()
  const onNavigate = vi.fn()
  const flushFrames = () =>
    act(() => {
      const pending = [...frames.values()]
      frames.clear()
      for (const frame of pending) frame(0)
    })
  function Harness({
    offset = 0,
    shift = 0,
    total = 200,
    resetKey = 'browse',
    reveal = undefined as HistoryWindowViewport['reveal'],
    scrollToTop = 0,
  }) {
    const root = useRef<HTMLDivElement>(null)
    const items = Array.from({ length: 50 }, (_, i) => ({ id: String(offset + i) }))
    const viewport: HistoryWindowViewport = {
      offset,
      total,
      onVisibleRange,
      onNavigate,
      resetKey,
      reveal,
      scrollToTop,
    }
    const spacing = useWindowedScroll(root, items, viewport)
    return (
      <div ref={root}>
        <div data-history-window-scroller data-shift={shift}>
          <div data-top style={{ height: spacing.top }} />
          {items.map(item => (
            <div key={item.id} data-item-id={item.id} />
          ))}
          <div style={{ height: spacing.bottom }} />
        </div>
      </div>
    )
  }
  beforeEach(() => {
    vi.clearAllMocks()
    frames = new Map()
    nextFrame = 0
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      frames.set(++nextFrame, callback)
      return nextFrame
    })
    vi.stubGlobal('cancelAnimationFrame', (id: number) => frames.delete(id))
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
      function (this: HTMLElement) {
        if (this.hasAttribute('data-item-id')) {
          const host = this.parentElement!
          const children = [...host.querySelectorAll('[data-item-id]')]
          const top =
            parseFloat((host.querySelector('[data-top]') as HTMLElement).style.height) || 0
          return rect(
            40 + top + children.indexOf(this) * 20 + Number(host.dataset.shift) - host.scrollTop,
            20
          )
        }
        return rect(40, 100)
      }
    )
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(100)
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(4000)
  })
  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('uses the existing ID anchor to compensate a window replacement and measurement error', () => {
    const { container, rerender } = render(<Harness />)
    flushFrames()
    flushFrames()
    const host = container.querySelector<HTMLElement>('[data-history-window-scroller]')!
    host.scrollTop = 200
    fireEvent.scroll(host)
    expect(onVisibleRange).toHaveBeenLastCalledWith(10, 14, true)
    rerender(<Harness offset={5} shift={7} />)
    expect(host.scrollTop).toBe(207)
    flushFrames()
    expect(onVisibleRange).toHaveBeenLastCalledWith(5, 9, false)
    host.scrollTop = 247
    fireEvent.scroll(host)
    rerender(<Harness offset={5} shift={7} />)
    expect(host.scrollTop).toBe(247)
  })

  it('restores a deep initial window into its global spacer position before measuring it', () => {
    const { container } = render(<Harness offset={110} />)
    flushFrames()
    flushFrames()
    const host = container.querySelector<HTMLElement>('[data-history-window-scroller]')!
    expect(host.scrollTop).toBe(2200)
    expect(onVisibleRange).toHaveBeenLastCalledWith(0, 4, false)
  })

  it('preserves native input arriving between layout compensation and the next frame', () => {
    const { container, rerender } = render(<Harness />)
    flushFrames()
    flushFrames()
    const host = container.querySelector<HTMLElement>('[data-history-window-scroller]')!
    host.scrollTop = 200
    fireEvent.scroll(host)
    rerender(<Harness offset={5} shift={7} />)
    expect(host.scrollTop).toBe(207)
    host.scrollTop += 40
    fireEvent.scroll(host)
    expect(onVisibleRange).toHaveBeenLastCalledWith(7, 11, true)
    rerender(<Harness offset={6} shift={7} />)
    expect(host.scrollTop).toBe(247)
    flushFrames()
    expect(host.scrollTop).toBe(247)
  })

  it('reports unloaded spacer indices relative to offset, with no stale anchor restore', () => {
    const { container, rerender } = render(<Harness />)
    flushFrames()
    flushFrames()
    const host = container.querySelector<HTMLElement>('[data-history-window-scroller]')!
    host.scrollTop = 2400
    fireEvent.scroll(host)
    expect(onVisibleRange).toHaveBeenLastCalledWith(120, 124, true)
    rerender(<Harness offset={110} />)
    expect(host.scrollTop).toBe(2400)
    flushFrames()
    expect(onVisibleRange).toHaveBeenLastCalledWith(10, 14, false)
  })

  it('resets the anchor and scroll position for a new query', () => {
    const { container, rerender } = render(<Harness />)
    flushFrames()
    flushFrames()
    const host = container.querySelector<HTMLElement>('[data-history-window-scroller]')!
    host.scrollTop = 200
    fireEvent.scroll(host)
    rerender(<Harness resetKey="filtered" />)
    expect(host.scrollTop).toBe(0)
  })

  it('reveals an explicit keyboard target in the replacement layout before reporting the viewport', () => {
    const { container, rerender } = render(<Harness />)
    flushFrames()
    flushFrames()
    const host = container.querySelector<HTMLElement>('[data-history-window-scroller]')!
    const row = host.querySelector<HTMLElement>('[data-item-id="5"]')!
    const revealRow = vi.fn(() => {
      host.scrollTop = 100
    })
    row.scrollIntoView = revealRow
    host.scrollTop = 500
    fireEvent.scroll(host)
    onVisibleRange.mockClear()
    const reveal = { id: '5' }
    rerender(<Harness reveal={reveal} />)
    expect(revealRow).toHaveBeenCalledOnce()
    expect(host.scrollTop).toBe(100)
    flushFrames()
    expect(onVisibleRange).toHaveBeenLastCalledWith(5, 9, false)
    rerender(<Harness reveal={reveal} />)
    expect(revealRow).toHaveBeenCalledOnce()
  })

  it('returns to the head after copy without restoring the old deep anchor', () => {
    const { container, rerender } = render(<Harness />)
    flushFrames()
    flushFrames()
    const host = container.querySelector<HTMLElement>('[data-history-window-scroller]')!
    host.scrollTop = 500
    fireEvent.scroll(host)
    rerender(<Harness scrollToTop={1} />)
    expect(host.scrollTop).toBe(0)
    flushFrames()
    expect(onVisibleRange).toHaveBeenLastCalledWith(0, 4, false)
  })

  it('navigates ten records only when native overflow is absent, without double movement', () => {
    const { container } = render(<Harness />)
    flushFrames()
    flushFrames()
    const host = container.querySelector<HTMLElement>('[data-history-window-scroller]')!
    fireEvent.wheel(host, { deltaY: 100 })
    fireEvent.keyDown(host.parentElement!, { key: 'PageDown' })
    expect(onNavigate).not.toHaveBeenCalled()
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(100)
    fireEvent.wheel(host, { deltaY: 100 })
    fireEvent.keyDown(host.parentElement!, { key: 'PageUp' })
    expect(onNavigate.mock.calls).toEqual([['older'], ['newer']])
  })

  it('delegates arrows for an evicted cursor to absolute selection navigation', async () => {
    const move = vi.fn().mockResolvedValue('neighbor')
    const viewport = { offset: 100, total: 200, onVisibleRange, onNavigate, onMoveSelection: move }
    expect(moveWindowSelection('ArrowDown', -1, 50, 3, viewport)).toBe(true)
    await act(async () => {})
    flushFrames()
    expect(move).toHaveBeenCalledWith(3)
    expect(moveWindowSelection('ArrowDown', 10, 50, 3, viewport)).toBe(false)
  })
})
