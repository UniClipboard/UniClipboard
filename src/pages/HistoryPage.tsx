import { m } from 'framer-motion'
import { Loader2, Search } from 'lucide-react'
import React, { useMemo, useState, useCallback, useRef, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter, filterToContentTypes } from '@/api/clipboardItems'
import { type SearchResultDto, type TimeRangePreset } from '@/api/daemon/search'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import { CompositeSearchBar, FilterBar } from '@/components/history/composite-search'
import HistoryCard from '@/components/history/HistoryCard'
import HistoryDetailSheet from '@/components/history/HistoryDetailSheet'
import { toast } from '@/components/ui/toast'
import { useClipboardEvents } from '@/hooks/useClipboardEvents'
import { useClipboardSearch } from '@/hooks/useClipboardSearch'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { useTransferProgress } from '@/hooks/useTransferProgress'
import type { ClipboardFileItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  copyToClipboard,
  removeClipboardItem,
  type PendingClipboardEntry,
} from '@/store/slices/clipboardSlice'

// ── Constants ───────────────────────────────────────────────────

/** Search-mode page size; the window grows by this as the user scrolls. */
const SEARCH_PAGE_SIZE = 100

/** Map a search-index content category to the display item's render type. */
function mapSearchContentType(ft: SearchResultDto['contentType']): DisplayClipboardItem['type'] {
  switch (ft) {
    case 'text':
      return 'text'
    case 'html':
      return 'code'
    case 'link':
      return 'link'
    case 'file':
      return 'file'
    case 'image':
      return 'image'
    case 'other':
      return 'unknown'
  }
}

// ── Helpers ─────────────────────────────────────────────────────

function formatBytesShort(bytes: number): string {
  if (bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const k = 1024
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), units.length - 1)
  const value = bytes / Math.pow(k, i)
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`
}

function buildPendingPreview(
  entry: PendingClipboardEntry,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  if (entry.totalBytes != null && entry.totalBytes > 0) {
    return t('clipboard.transfer.incomingWithSize', { size: formatBytesShort(entry.totalBytes) })
  }
  return t('clipboard.transfer.incoming')
}

function buildPendingFileContent(entry: PendingClipboardEntry): ClipboardFileItem | null {
  if (entry.filenames.length === 0) return null
  const fileSizes: number[] =
    entry.filenames.length === 1 && entry.totalBytes != null && entry.totalBytes > 0
      ? [entry.totalBytes]
      : entry.filenames.map(() => -1)
  return { file_names: entry.filenames, file_sizes: fileSizes }
}

// ── Page ────────────────────────────────────────────────────────

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const [activeFilter, setActiveFilter] = useState<Filter>(Filter.All)
  const [searchQuery, setSearchQuery] = useState('')
  // `searchQuery` is the raw input value; `submittedQuery` is what was actually
  // sent to the search engine. It is auto-submitted (debounced) as the user
  // types, and submitted immediately on Enter. Clearing the input resets it.
  const [submittedQuery, setSubmittedQuery] = useState('')
  const [timeRange, setTimeRange] = useState<TimeRangePreset>('all_time')
  const [sourceFilter, setSourceFilter] = useState<string | null>(null)
  // Search-mode pagination: the engine is queried for a growing window
  // (`offset` stays 0, `limit` grows) so scrolling reveals more matches. Reset
  // to one page whenever the query/filters change.
  const [searchLimit, setSearchLimit] = useState(SEARCH_PAGE_SIZE)
  const searchInputRef = useRef<HTMLInputElement>(null)
  const [hoveredId, setHoveredId] = useState<string | null>(null)
  const [copySuccessId, setCopySuccessId] = useState<string | null>(null)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [promotedId, setPromotedId] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const deleteTargetRef = useRef<string | null>(null)
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  // Stable Set of ids already rendered once; read during render to gate the
  // entrance animation, mutated only in an effect (never during render).
  const [seenIds] = useState(() => new Set<string>())

  useShortcutScope('clipboard')
  const { hasMore, handleLoadMore } = useClipboardEvents(activeFilter)

  // Activate file-transfer progress event listener for this page
  useTransferProgress()

  const items = useAppSelector(state => state.clipboard.items)
  const pendingItems = useAppSelector(state => state.clipboard.pendingItems)
  const spaceMembers = useAppSelector(state => state.devices.spaceMembers)
  const mobileDevices = useMobileDeviceList()

  const deviceNameByPeerId = useMemo(() => {
    const map: Record<string, string> = {}
    for (const m of spaceMembers) map[m.peerId] = m.deviceName
    return map
  }, [spaceMembers])

  // Source-filter options: P2P space members + mobile-sync devices. Mobile ids
  // are prefixed to match the `mobile_sync:<id>` value stored as the clipboard
  // event's source_device on the backend.
  const sourceOptions = useMemo(
    () => [
      ...spaceMembers.map(m => ({ id: m.peerId, name: m.deviceName, kind: 'p2p' as const })),
      ...mobileDevices.map(d => ({
        id: `mobile_sync:${d.deviceId}`,
        name: d.label,
        kind: 'mobile' as const,
      })),
    ],
    [spaceMembers, mobileDevices]
  )

  const displayItems = useMemo<DisplayClipboardItem[]>(() => {
    const realItems = items.map(entry => ({
      id: entry.id,
      type: entry.type,
      content: entry.content,
      activeTime: entry.activeTime,
      isFavorited: entry.isFavorited,
      isUnavailable: entry.isUnavailable,
    }))

    const realIds = new Set(realItems.map(it => it.id))
    const pendingDisplayItems: DisplayClipboardItem[] = pendingItems.flatMap(p =>
      realIds.has(p.entryId)
        ? []
        : [
            {
              id: p.entryId,
              type: 'file' as const,
              activeTime: p.createdAt,
              content: buildPendingFileContent(p),
              device: deviceNameByPeerId[p.fromDevice],
              textPreview: buildPendingPreview(p, t),
            },
          ]
    )

    return [...pendingDisplayItems, ...realItems]
  }, [items, pendingItems, deviceNameByPeerId, t])

  // ── Server-side search ────────────────────────────────────────
  // Any active filter switches to the search engine. The browse LIST endpoint
  // does NOT honor the content-type/source/time filters (see clipboardSlice:
  // `filter` is dropped before the request), so a content-type selection alone
  // must go through search — only that path actually narrows results. Browse
  // mode (with live insertion + infinite scroll) is reserved for the unfiltered
  // view; clearing every filter returns to it.
  const hasTypeFilter = activeFilter !== Filter.All && activeFilter !== Filter.Favorited
  const hasTimeFilter = timeRange !== 'all_time'
  const hasSourceFilter = sourceFilter !== null
  const isSearchActive =
    submittedQuery.trim().length > 0 || hasTypeFilter || hasTimeFilter || hasSourceFilter

  // Auto-submit while typing (debounced); clearing the input drops straight
  // back to browse mode. Enter bypasses the debounce via the input handler.
  useEffect(() => {
    const q = searchQuery.trim()
    if (!q) {
      setSubmittedQuery('')
      return
    }
    const timer = setTimeout(() => setSubmittedQuery(q), 800)
    return () => clearTimeout(timer)
  }, [searchQuery])

  // Map a raw search hit to a renderable history card.
  const mapSearchResult = useCallback(
    (r: SearchResultDto): DisplayClipboardItem => ({
      id: r.entryId,
      type: mapSearchContentType(r.contentType),
      activeTime: r.activeTimeMs,
      content: null,
      textPreview: r.textPreview ?? undefined,
    }),
    []
  )

  // Collapse the window back to one page whenever the search inputs change so a
  // new query never inherits the previous query's expanded limit.
  useEffect(() => {
    setSearchLimit(SEARCH_PAGE_SIZE)
  }, [submittedQuery, activeFilter, sourceFilter, timeRange])

  const {
    results: searchResults,
    isSearching: searchLoading,
    total: searchTotal,
  } = useClipboardSearch(
    {
      enabled: isSearchActive,
      query: submittedQuery.trim(),
      contentTypes: filterToContentTypes(activeFilter),
      sourceDevices: sourceFilter ?? undefined,
      timePreset: hasTimeFilter ? timeRange : undefined,
      limit: searchLimit,
    },
    mapSearchResult
  )

  // In search mode show engine results; otherwise the browse (paginated) list.
  const baseItems = useMemo<DisplayClipboardItem[]>(
    () => (isSearchActive ? (searchResults ?? []) : displayItems),
    [isSearchActive, searchResults, displayItems]
  )

  const orderedItems = useMemo(() => {
    if (!promotedId) return baseItems
    const idx = baseItems.findIndex(it => it.id === promotedId)
    if (idx <= 0) return baseItems
    return [baseItems[idx], ...baseItems.slice(0, idx), ...baseItems.slice(idx + 1)]
  }, [baseItems, promotedId])

  // ── Infinite scroll ───────────────────────────────────────────
  const scrollRef = useRef<HTMLDivElement>(null)
  const hasMoreRef = useRef(hasMore)
  const handleLoadMoreRef = useRef(handleLoadMore)
  const isSearchActiveRef = useRef(isSearchActive)
  // Search-mode load-more reads these through refs so the scroll handler can stay
  // a stable, dependency-free callback (same pattern as the browse-mode refs).
  const searchTotalRef = useRef(searchTotal)
  const searchLoadedRef = useRef(0)
  const searchLoadingRef = useRef(searchLoading)

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
    searchTotalRef.current = searchTotal
    searchLoadedRef.current = searchResults?.length ?? 0
    searchLoadingRef.current = searchLoading
  }, [searchTotal, searchResults, searchLoading])

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
        setSearchLimit(n => n + SEARCH_PAGE_SIZE)
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
  }, [orderedItems, checkShouldLoadMore])

  // ── Copy handler ──────────────────────────────────────────────
  const handleCopy = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await dispatch(copyToClipboard(id)).unwrap()
        if (copyTimerRef.current) clearTimeout(copyTimerRef.current)
        setCopySuccessId(id)
        setPromotedId(id)
        copyTimerRef.current = setTimeout(() => setCopySuccessId(null), 1200)
        return true
      } catch {
        toast.error(t('clipboard.errors.copyFailed'))
        return false
      }
    },
    [dispatch, t]
  )

  // ── Delete handler ────────────────────────────────────────────
  const handleConfirmDelete = useCallback(async () => {
    const targetId = deleteTargetRef.current
    if (!targetId) return
    setDeletingId(targetId)
    setTimeout(async () => {
      try {
        await dispatch(removeClipboardItem(targetId)).unwrap()
      } catch {
        toast.error(t('clipboard.errors.deleteFailed', 'Delete failed'))
      }
      setDeletingId(null)
      deleteTargetRef.current = null
    }, 400)
  }, [dispatch, t])

  const handleDelete = useCallback((id: string) => {
    deleteTargetRef.current = id
    setDeleteDialogOpen(true)
  }, [])

  // ── Hover keyboard shortcuts ──────────────────────────────────
  useShortcut({
    key: 'c',
    scope: 'clipboard',
    enabled: hoveredId !== null,
    handler: () => {
      if (hoveredId) handleCopy(hoveredId)
    },
    preventDefault: false,
  })

  useShortcut({
    key: 'd',
    scope: 'clipboard',
    enabled: hoveredId !== null,
    handler: () => {
      if (hoveredId) handleDelete(hoveredId)
    },
    preventDefault: false,
  })

  // CMD/Ctrl+F focuses the search box (works even while another input is focused).
  useShortcut({
    key: 'mod+f',
    scope: 'clipboard',
    handler: () => {
      const el = searchInputRef.current
      if (!el) return
      el.focus()
      el.select()
    },
    enableOnFormTags: true,
    preventDefault: true,
  })

  // Record rendered ids after commit so subsequent remounts (e.g. column
  // shifts when a new item is prepended) skip the entrance animation.
  useEffect(() => {
    for (const item of orderedItems) seenIds.add(item.id)
  }, [orderedItems, seenIds])

  const selectedItem = useMemo(
    () => orderedItems.find(it => it.id === selectedId) ?? null,
    [orderedItems, selectedId]
  )

  const handleCardClick = useCallback((id: string) => setSelectedId(id), [])

  const handleSheetDelete = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await dispatch(removeClipboardItem(id)).unwrap()
        return true
      } catch {
        toast.error(t('clipboard.errors.deleteFailed', 'Delete failed'))
        return false
      }
    },
    [dispatch, t]
  )

  const totalCount = displayItems.length

  return (
    <div className="flex flex-col h-full">
      {/* ── Toolbar: quick filters (left) + composite search (right) ─ */}
      <div className="shrink-0 flex flex-wrap items-center gap-x-3 gap-y-2 px-2 pt-3 pb-2">
        <FilterBar
          contentFilter={activeFilter}
          sourceFilter={sourceFilter}
          timeRange={timeRange}
          onContentFilterChange={setActiveFilter}
          onSourceFilterChange={setSourceFilter}
          onTimeRangeChange={setTimeRange}
          sourceOptions={sourceOptions}
        />
        <div className="ml-auto w-80 max-w-full">
          <CompositeSearchBar
            contentFilter={activeFilter}
            sourceFilter={sourceFilter}
            timeRange={timeRange}
            onContentFilterChange={setActiveFilter}
            onSourceFilterChange={setSourceFilter}
            onTimeRangeChange={setTimeRange}
            onQueryChange={setSearchQuery}
            onQuerySubmit={text => setSubmittedQuery(text.trim())}
            sourceOptions={sourceOptions}
            totalCount={totalCount}
            inputRef={searchInputRef}
          />
        </div>
      </div>

      {/* ── Grid ───────────────────────────────────────────────── */}
      <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto">
        {searchLoading && orderedItems.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 pb-10">
            <Loader2 className="size-5 text-muted-foreground/40 animate-spin" />
            <p className="text-[12px] text-muted-foreground/50">
              {t('clipboard.search.searching')}
            </p>
          </div>
        ) : orderedItems.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 pb-10">
            <div className="size-12 rounded-2xl bg-muted/30 flex items-center justify-center">
              <Search className="size-5 text-muted-foreground/30" />
            </div>
            <div className="text-center space-y-1">
              {isSearchActive ? (
                <>
                  <p className="text-[13px] font-medium">
                    {submittedQuery.trim()
                      ? t('clipboard.search.noResults', { query: submittedQuery })
                      : t('clipboard.search.noResultsFiltered')}
                  </p>
                  <p className="text-[12px] text-muted-foreground/50">
                    {t('clipboard.search.noResultsSub')}
                  </p>
                </>
              ) : (
                <>
                  <p className="text-[13px] font-medium">
                    {t('clipboard.content.noClipboardItems')}
                  </p>
                  <p className="text-[12px] text-muted-foreground/50">
                    {t('clipboard.content.emptyDescription')}
                  </p>
                </>
              )}
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] items-start gap-x-3 gap-y-2 px-3 pt-2 pb-4">
            {orderedItems.map(item => {
              const isNew = !seenIds.has(item.id)
              return (
                <m.div
                  key={item.id}
                  initial={isNew ? { opacity: 0, y: 16 } : false}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                  className="h-44 rounded-xl border border-border/40 bg-card/40 overflow-hidden"
                >
                  <HistoryCard
                    item={item}
                    isHovered={hoveredId === item.id}
                    copySuccess={copySuccessId === item.id}
                    isDeleting={deletingId === item.id}
                    onCopy={handleCopy}
                    onClick={handleCardClick}
                    onHoverChange={setHoveredId}
                  />
                </m.div>
              )
            })}
          </div>
        )}
      </div>

      <HistoryDetailSheet
        item={selectedItem}
        open={selectedId !== null}
        onOpenChange={open => {
          if (!open) setSelectedId(null)
        }}
        onCopy={handleCopy}
        onDelete={handleSheetDelete}
      />

      <DeleteConfirmDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        onConfirm={handleConfirmDelete}
        count={1}
      />
    </div>
  )
}

export default HistoryPage
