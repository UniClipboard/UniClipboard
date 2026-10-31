import { useEffect, useEffectEvent, useLayoutEffect, useRef, type RefObject } from 'react'
import type { HistoryWindowViewport } from './contract'

export function revealWindowItem(root: HTMLDivElement | null, id: string): void {
  Array.from(root?.querySelectorAll<HTMLElement>('[data-item-id]') ?? [])
    .find(row => row.dataset.itemId === id)
    ?.scrollIntoView({ block: 'nearest' })
}

/** Keep a visible record at the same pixel position when its bounded data window changes. */
export function useWindowedScroll(
  root: RefObject<HTMLDivElement | null>,
  items: { id: string }[],
  viewport: HistoryWindowViewport | undefined
): void {
  const anchor = useRef<{ id: string; top: number } | null>(null)
  const restoring = useRef(false)
  const enabled = viewport !== undefined
  const itemIds = JSON.stringify(items.map(item => item.id))
  const scroller = () =>
    root.current?.querySelector<HTMLElement>('[data-history-window-scroller]') ?? root.current
  const measure = useEffectEvent((scrolling = false) => {
    const host = root.current
    const scroll = scroller()
    if (!host || !scroll || restoring.current || !viewport) return
    const bounds = scroll.getBoundingClientRect()
    const rows = Array.from(host.querySelectorAll<HTMLElement>('[data-item-id]')).filter(row => {
      const rect = row.getBoundingClientRect()
      return rect.bottom > bounds.top && rect.top < bounds.bottom
    })
    if (!rows.length) return
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
  useEffect(() => {
    const host = root.current
    if (!host || !enabled) return
    const onScroll = () => measure(true)
    host.addEventListener('scroll', onScroll, true)
    const resize = new ResizeObserver(() => measure())
    resize.observe(host)
    const mutations = new MutationObserver(() => measure())
    mutations.observe(host, { childList: true, subtree: true })
    const frame = requestAnimationFrame(() => measure())
    return () => {
      host.removeEventListener('scroll', onScroll, true)
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
    } else anchor.current = null
  })
  useLayoutEffect(() => {
    if (!enabled) return
    restore()
    const frame = requestAnimationFrame(() => {
      restoring.current = false
      measure()
    })
    return () => {
      cancelAnimationFrame(frame)
      restoring.current = false
    }
  }, [itemIds, enabled])
}
