import { useCallback, useEffect, useRef } from 'react'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

interface UseHistoryInfiniteScrollArgs {
  isSearchActive: boolean
  hasMore: boolean
  handleLoadMore: () => void
  searchTotal: number | null
  searchLoadedCount: number
  searchLoading: boolean
  growSearchWindow: () => void
  /** Re-checked after this list changes (a prepend can leave the viewport short). */
  items: DisplayClipboardItem[]
}

/**
 * Infinite-scroll driver for the history grid. The scroll handler stays a
 * stable, dependency-free callback that reads every dynamic value through refs,
 * so the scroll/resize listeners subscribe once instead of re-binding on each
 * render. In search mode it grows the engine window; in browse mode it asks the
 * paginated list for the next page.
 */
export function useHistoryInfiniteScroll({
  isSearchActive,
  hasMore,
  handleLoadMore,
  searchTotal,
  searchLoadedCount,
  searchLoading,
  growSearchWindow,
  items,
}: UseHistoryInfiniteScrollArgs) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const hasMoreRef = useRef(hasMore)
  const handleLoadMoreRef = useRef(handleLoadMore)
  const isSearchActiveRef = useRef(isSearchActive)
  const searchTotalRef = useRef(searchTotal)
  const searchLoadedRef = useRef(searchLoadedCount)
  const searchLoadingRef = useRef(searchLoading)
  const growSearchWindowRef = useRef(growSearchWindow)

  useEffect(() => {
    hasMoreRef.current = hasMore
  }, [hasMore])

  useEffect(() => {
    isSearchActiveRef.current = isSearchActive
  }, [isSearchActive])

  useEffect(() => {
    handleLoadMoreRef.current = handleLoadMore
  }, [handleLoadMore])

  useEffect(() => {
    growSearchWindowRef.current = growSearchWindow
  }, [growSearchWindow])

  useEffect(() => {
    searchTotalRef.current = searchTotal
    searchLoadedRef.current = searchLoadedCount
    searchLoadingRef.current = searchLoading
  }, [searchTotal, searchLoadedCount, searchLoading])

  const checkShouldLoadMore = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const { scrollTop, scrollHeight, clientHeight } = el
    if (scrollHeight - scrollTop - clientHeight >= 400) return
    if (isSearchActiveRef.current) {
      // Grow the search window while a fetch isn't already in flight and the
      // engine reported more matches than we currently hold.
      const total = searchTotalRef.current
      if (!searchLoadingRef.current && total != null && searchLoadedRef.current < total) {
        searchLoadingRef.current = true // guard against re-firing before state settles
        growSearchWindowRef.current()
      }
    } else if (hasMoreRef.current) {
      handleLoadMoreRef.current()
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
