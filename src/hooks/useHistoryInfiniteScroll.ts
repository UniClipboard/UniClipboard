import { useCallback, useEffect, useRef } from 'react'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

interface UseHistoryInfiniteScrollArgs {
  /** Whether the engine reported more matches than the current window holds. */
  hasMore: boolean
  /** Whether a window fetch is already in flight (guards against re-firing). */
  isLoading: boolean
  /** Grow the live-list window by one page. */
  loadMore: () => void
  /** Re-checked after this list changes (a prepend can leave the viewport short). */
  items: DisplayClipboardItem[]
}

/**
 * Infinite-scroll driver for the history grid. The scroll handler stays a
 * stable, dependency-free callback that reads every dynamic value through refs,
 * so the scroll/resize listeners subscribe once instead of re-binding on each
 * render. Browse and search share one window now, so it just grows the live
 * list whenever the viewport nears the bottom and more matches remain.
 */
export function useHistoryInfiniteScroll({
  hasMore,
  isLoading,
  loadMore,
  items,
}: UseHistoryInfiniteScrollArgs) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const hasMoreRef = useRef(hasMore)
  const isLoadingRef = useRef(isLoading)
  const loadMoreRef = useRef(loadMore)

  useEffect(() => {
    hasMoreRef.current = hasMore
  }, [hasMore])

  useEffect(() => {
    isLoadingRef.current = isLoading
  }, [isLoading])

  useEffect(() => {
    loadMoreRef.current = loadMore
  }, [loadMore])

  const checkShouldLoadMore = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const { scrollTop, scrollHeight, clientHeight } = el
    if (scrollHeight - scrollTop - clientHeight >= 400) return
    if (hasMoreRef.current && !isLoadingRef.current) {
      isLoadingRef.current = true // guard against re-firing before isLoading settles
      loadMoreRef.current()
    }
  }, [])

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    el.addEventListener('scroll', checkShouldLoadMore, { passive: true })
    const observer = new ResizeObserver(checkShouldLoadMore)
    observer.observe(el)
    return () => {
      el.removeEventListener('scroll', checkShouldLoadMore)
      observer.disconnect()
    }
  }, [checkShouldLoadMore])

  useEffect(() => {
    checkShouldLoadMore()
  }, [items, checkShouldLoadMore])

  return scrollRef
}
