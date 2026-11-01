import { useEffect, useEffectEvent, useLayoutEffect, useRef, useState, type RefObject } from 'react'
import type { HistoryWindowViewport } from '@/ui-kit/contract'
import { absoluteVisibleRange, windowedGeometry } from '@/ui-kit/windowed-geometry'

export function revealWindowItem(root: HTMLDivElement | null, id: string): void {
  Array.from(root?.querySelectorAll<HTMLElement>('[data-item-id]') ?? [])
    .find(row => row.dataset.itemId === id)
    ?.scrollIntoView({ block: 'nearest' })
}

/** Keep a visible record at the same pixel position when its bounded data window changes. */
export function useWindowedScroll(
  root: RefObject<HTMLDivElement | null>,
  items: { id: string }[],
  viewport: HistoryWindowViewport | undefined,
  layout: { columns?: number; rowHeightRem?: number; key?: string } = {}
) {
  const columns = Math.max(1, layout.columns ?? 1)
  const layoutKey = `${columns}:${layout.key ?? ''}`
  const [estimate, setEstimate] = useState<{ key: string; height: number } | null>(null)
  const rem =
    typeof document === 'undefined'
      ? 16
      : parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
  const rowHeight = estimate?.key === layoutKey ? estimate.height : (layout.rowHeightRem ?? 4) * rem
  const geometry = windowedGeometry(
    viewport?.offset ?? 0,
    items.length,
    viewport?.total ?? items.length,
    rowHeight,
    columns
  )
  const initialized = useRef(false)
  const anchor = useRef<{ id: string; top: number } | null>(null)
  const restoring = useRef(false)
  const restoredTop = useRef(0)
  const resetKey = useRef(viewport?.resetKey)
  const headRevision = useRef(viewport?.scrollToTop)
  const revealed = useRef<HistoryWindowViewport['reveal']>(undefined)
  const enabled = viewport !== undefined
  const itemIds = JSON.stringify(items.map(item => item.id))
  const scroller = () =>
    root.current?.querySelector<HTMLElement>('[data-history-window-scroller]') ?? root.current
  const measure = useEffectEvent((scrolling = false) => {
    const host = root.current
    const scroll = scroller()
    if (!host || !scroll || !viewport) return
    if (restoring.current) {
      // Ignore our own compensation event, but capture native input that
      // arrives before the next frame instead of restoring over that input.
      if (!scrolling || Math.abs(scroll.scrollTop - restoredTop.current) < 0.5) return
      restoring.current = false
    }
    const bounds = scroll.getBoundingClientRect()
    const allRows = Array.from(host.querySelectorAll<HTMLElement>('[data-item-id]'))
    // Freeze a measured pitch for this layout. Re-estimating on each eviction
    // amplifies date-header and partial-row noise across the full result count.
    if (estimate?.key !== layoutKey && allRows.length) {
      const tops = [...new Set(allRows.map(row => row.getBoundingClientRect().top))].sort(
        (a, b) => a - b
      )
      const pitches = tops
        .slice(1)
        .map((top, i) => top - tops[i])
        .filter(pitch => pitch > 0)
        .sort((a, b) => a - b)
      const height = pitches.length
        ? pitches[Math.floor(pitches.length / 2)]
        : allRows[0].getBoundingClientRect().height
      if (height > 0) setEstimate({ key: layoutKey, height })
    }
    const rows = allRows.filter(row => {
      const rect = row.getBoundingClientRect()
      return rect.bottom > bounds.top && rect.top < bounds.bottom
    })
    if (!rows.length) {
      if (scrolling) anchor.current = null
      const range = absoluteVisibleRange(
        scroll.scrollTop,
        scroll.clientHeight,
        rowHeight,
        viewport.total,
        columns
      )
      viewport.onVisibleRange(range.start - viewport.offset, range.end - viewport.offset, scrolling)
      return
    }
    const ids = items.map(item => item.id)
    const first = rows[0]
    // Passive DOM changes must not replace the record selected by the user's last scroll.
    if (scrolling || !anchor.current || !ids.includes(anchor.current.id)) {
      anchor.current = {
        id: first.dataset.itemId!,
        top: first.getBoundingClientRect().top - bounds.top,
      }
    }
    const indices = rows.map(row => ids.indexOf(row.dataset.itemId!)).filter(index => index >= 0)
    if (indices.length)
      viewport.onVisibleRange(Math.min(...indices), Math.max(...indices), scrolling)
  })
  const navigateWithoutOverflow = useEffectEvent((direction: 'newer' | 'older') => {
    const scroll = scroller()
    if (!scroll || scroll.scrollHeight > scroll.clientHeight + 1) return false
    viewport?.onNavigate(direction)
    return true
  })
  useEffect(() => {
    const host = root.current
    if (!host || !enabled) return
    const onScroll = () => measure(true)
    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey || Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return
      if (navigateWithoutOverflow(event.deltaY > 0 ? 'older' : 'newer')) event.preventDefault()
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return
      if (event.key !== 'PageDown' && event.key !== 'PageUp') return
      if (navigateWithoutOverflow(event.key === 'PageDown' ? 'older' : 'newer'))
        event.preventDefault()
    }
    host.addEventListener('scroll', onScroll, true)
    host.addEventListener('wheel', onWheel, { passive: false })
    host.addEventListener('keydown', onKeyDown)
    const resize = new ResizeObserver(() => measure())
    resize.observe(host)
    const mutations = new MutationObserver(() => measure())
    mutations.observe(host, { childList: true, subtree: true })
    const frame = requestAnimationFrame(() => measure())
    return () => {
      host.removeEventListener('scroll', onScroll, true)
      host.removeEventListener('wheel', onWheel)
      host.removeEventListener('keydown', onKeyDown)
      resize.disconnect()
      mutations.disconnect()
      cancelAnimationFrame(frame)
    }
  }, [root, enabled])
  const restore = useEffectEvent(() => {
    const scroll = scroller()
    const saved = anchor.current
    const row = Array.from(
      root.current?.querySelectorAll<HTMLElement>('[data-item-id]') ?? []
    ).find(row => row.dataset.itemId === saved?.id)
    if (saved && row && scroll) {
      restoring.current = true
      scroll.scrollTop +=
        row.getBoundingClientRect().top - scroll.getBoundingClientRect().top - saved.top
      restoredTop.current = scroll.scrollTop
    } else anchor.current = null
  })
  useLayoutEffect(() => {
    if (!enabled) return
    if (!initialized.current) {
      initialized.current = true
      const scroll = scroller()
      if (scroll) scroll.scrollTop = geometry.top
    } else if (
      resetKey.current !== viewport?.resetKey ||
      headRevision.current !== viewport?.scrollToTop
    ) {
      anchor.current = null
      resetKey.current = viewport?.resetKey
      headRevision.current = viewport?.scrollToTop
      const scroll = scroller()
      if (scroll) scroll.scrollTop = 0
    } else if (viewport?.reveal && revealed.current !== viewport.reveal) {
      anchor.current = null
      revealWindowItem(root.current, viewport.reveal.id)
    } else restore()
    revealed.current = viewport?.reveal
    const frame = requestAnimationFrame(() => {
      restoring.current = false
      measure()
    })
    return () => {
      cancelAnimationFrame(frame)
      restoring.current = false
    }
  }, [
    itemIds,
    enabled,
    geometry.top,
    columns,
    viewport?.resetKey,
    viewport?.scrollToTop,
    viewport?.reveal,
  ])
  return { ...geometry, rowHeight, columns }
}

/** Handle an arrow outside the loaded window through the controller's absolute cursor. */
export function moveWindowSelection(
  key: string,
  index: number,
  length: number,
  columns: number,
  viewport: HistoryWindowViewport | undefined
): boolean {
  if (!viewport?.onMoveSelection) return false
  const steps: Record<string, number> = {
    ArrowUp: -columns,
    ArrowDown: columns,
    ArrowLeft: -1,
    ArrowRight: 1,
  }
  const movement = key === 'Home' ? 'first' : key === 'End' ? 'last' : steps[key]
  if (movement === undefined) return false
  if (
    typeof movement === 'number' &&
    index >= 0 &&
    index + movement >= 0 &&
    index + movement < length
  )
    return false
  void viewport.onMoveSelection(movement)
  return true
}

/** Match a responsive CSS grid's column count to its logical row geometry. */
export function useGridColumns(
  root: RefObject<HTMLDivElement | null>,
  cardRem: number,
  gapRem: number,
  paddingRem: number
) {
  const [width, setWidth] = useState(0)
  useEffect(() => {
    const host = root.current
    if (!host) return
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width))
    observer.observe(host)
    return () => observer.disconnect()
  }, [root])
  const rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
  return Math.max(
    1,
    Math.floor((width - paddingRem * rem + gapRem * rem) / ((cardRem + gapRem) * rem))
  )
}
