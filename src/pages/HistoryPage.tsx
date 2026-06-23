import { m } from 'framer-motion'
import {
  Check,
  ChevronDown,
  Clock,
  Code,
  ExternalLink,
  File,
  FileText,
  Image as ImageIcon,
  Laptop,
  Loader2,
  Search,
} from 'lucide-react'
import React, { useMemo, useState, useCallback, useRef, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter, filterToContentTypes } from '@/api/clipboardItems'
import { type SearchResultDto, type TimeRangePreset } from '@/api/daemon/search'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import HistoryCard from '@/components/history/HistoryCard'
import HistoryDetailSheet from '@/components/history/HistoryDetailSheet'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { toast } from '@/components/ui/toast'
import { useClipboardEvents } from '@/hooks/useClipboardEvents'
import { useClipboardSearch } from '@/hooks/useClipboardSearch'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { useTransferProgress } from '@/hooks/useTransferProgress'
import type { ClipboardFileItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  copyToClipboard,
  removeClipboardItem,
  type PendingClipboardEntry,
} from '@/store/slices/clipboardSlice'

// ── Constants ───────────────────────────────────────────────────

/** Time-range presets shown in the toolbar dropdown (order = display order). */
const TIME_RANGES: TimeRangePreset[] = [
  'all_time',
  'today',
  'yesterday',
  'last_7d',
  'last_30d',
  'this_week',
  'this_month',
]

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

const FILTER_TABS: { key: Filter; labelKey: string; icon?: React.ElementType }[] = [
  { key: Filter.All, labelKey: 'history.filter.all' },
  { key: Filter.Text, labelKey: 'history.filter.text', icon: FileText },
  { key: Filter.Code, labelKey: 'history.filter.code', icon: Code },
  { key: Filter.Link, labelKey: 'history.filter.link', icon: ExternalLink },
  { key: Filter.Image, labelKey: 'history.filter.image', icon: ImageIcon },
  { key: Filter.File, labelKey: 'history.filter.file', icon: File },
]

// ── Masonry distribution ──────────────────────────────────────

const MASONRY_BREAKPOINTS: [number, number][] = [
  [500, 1],
  [750, 2],
  [1050, 3],
  [1400, 4],
]
const MASONRY_DEFAULT_COLS = 5

function useColumnCount(): number {
  const [cols, setCols] = useState(() => {
    const w = window.innerWidth
    for (const [bp, c] of MASONRY_BREAKPOINTS) {
      if (w <= bp) return c
    }
    return MASONRY_DEFAULT_COLS
  })
  useEffect(() => {
    const onResize = () => {
      const w = window.innerWidth
      let next = MASONRY_DEFAULT_COLS
      for (const [bp, c] of MASONRY_BREAKPOINTS) {
        if (w <= bp) {
          next = c
          break
        }
      }
      setCols(next)
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])
  return cols
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

  const deviceNameByPeerId = useMemo(() => {
    const map: Record<string, string> = {}
    for (const m of spaceMembers) map[m.peerId] = m.deviceName
    return map
  }, [spaceMembers])

  const formatRelativeTime = useCallback(
    (timestamp: number): string => {
      const diff = Date.now() - timestamp
      const minutes = Math.floor(diff / 60000)
      if (minutes < 1) return t('clipboard.time.justNow')
      if (minutes < 60) return t('clipboard.time.minutesAgo', { minutes })
      const hours = Math.floor(minutes / 60)
      if (hours < 24) return t('clipboard.time.hoursAgo', { hours })
      return t('clipboard.time.daysAgo', { days: Math.floor(hours / 24) })
    },
    [t]
  )

  const displayItems = useMemo<DisplayClipboardItem[]>(() => {
    const realItems = items.map(entry => ({
      id: entry.id,
      type: entry.type,
      content: entry.content,
      time: formatRelativeTime(entry.activeTime),
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
              time: t('clipboard.time.justNow'),
              activeTime: p.createdAt,
              content: buildPendingFileContent(p),
              device: deviceNameByPeerId[p.fromDevice],
              textPreview: buildPendingPreview(p, t),
            },
          ]
    )

    return [...pendingDisplayItems, ...realItems]
  }, [items, pendingItems, deviceNameByPeerId, formatRelativeTime, t])

  // ── Server-side search ────────────────────────────────────────
  // Search mode is driven by the submitted query OR an active time filter. The
  // content-type filter alone keeps browse mode (paginated via useClipboardEvents);
  // in search mode it is applied as an additional contentTypes constraint.
  const hasTimeFilter = timeRange !== 'all_time'
  const hasSourceFilter = sourceFilter !== null
  const isSearchActive = submittedQuery.trim().length > 0 || hasTimeFilter || hasSourceFilter

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
      time: formatRelativeTime(r.activeTimeMs),
      activeTime: r.activeTimeMs,
      content: null,
      textPreview: r.textPreview ?? undefined,
    }),
    [formatRelativeTime]
  )

  const { results: searchResults, isSearching: searchLoading } = useClipboardSearch(
    {
      enabled: isSearchActive,
      query: submittedQuery.trim(),
      contentTypes: filterToContentTypes(activeFilter),
      sourceDevices: sourceFilter ?? undefined,
      timePreset: hasTimeFilter ? timeRange : undefined,
      limit: 100,
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

  useEffect(() => {
    hasMoreRef.current = hasMore
  }, [hasMore])

  useEffect(() => {
    isSearchActiveRef.current = isSearchActive
  }, [isSearchActive])

  useEffect(() => {
    handleLoadMoreRef.current = handleLoadMore
  }, [handleLoadMore])

  const checkShouldLoadMore = useCallback(() => {
    const el = scrollRef.current
    if (!el || !hasMoreRef.current || isSearchActiveRef.current) return
    const { scrollTop, scrollHeight, clientHeight } = el
    if (scrollHeight - scrollTop - clientHeight < 400) {
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

  const columnCount = useColumnCount()
  const columns = useMemo(() => {
    const cols: DisplayClipboardItem[][] = Array.from({ length: columnCount }, () => [])
    for (let i = 0; i < orderedItems.length; i++) {
      cols[i % columnCount].push(orderedItems[i])
    }
    return cols
  }, [orderedItems, columnCount])

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
      {/* ── Toolbar: filters (left) + search (right) ───────────── */}
      <div className="shrink-0 flex items-center justify-between gap-2 px-5 py-3 border-b border-border/20">
        {/* Filters */}
        <div className="flex items-center gap-2">
          {FILTER_TABS.map(tab => {
            const isActive = activeFilter === tab.key
            return (
              <button
                key={tab.key}
                type="button"
                onClick={() => setActiveFilter(tab.key)}
                className={cn(
                  'flex items-center gap-1.5 h-7 px-3 rounded-full text-[12px] font-medium whitespace-nowrap transition-all duration-150',
                  isActive
                    ? 'bg-foreground/8 text-foreground'
                    : 'text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted/40'
                )}
              >
                {tab.icon && <tab.icon className="size-3" />}
                {t(tab.labelKey)}
                {tab.key === Filter.All && totalCount > 0 && (
                  <span className="text-[10px] tabular-nums text-muted-foreground/40 ml-0.5">
                    {totalCount}
                  </span>
                )}
              </button>
            )
          })}
        </div>

        {/* Time filter + search (right side) */}
        <div className="flex items-center gap-2 shrink-0">
          {/* Source device */}
          {spaceMembers.length > 0 && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  aria-label={t('history.source.label')}
                  className={cn(
                    'flex items-center gap-1.5 h-7 px-3 rounded-full text-[12px] font-medium whitespace-nowrap transition-all duration-150',
                    hasSourceFilter
                      ? 'bg-foreground/8 text-foreground'
                      : 'text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted/40'
                  )}
                >
                  <Laptop className="size-3" />
                  {sourceFilter
                    ? (deviceNameByPeerId[sourceFilter] ?? t('history.source.label'))
                    : t('history.source.all')}
                  <ChevronDown className="size-3 opacity-50" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-44">
                <DropdownMenuItem
                  onClick={() => setSourceFilter(null)}
                  className="flex items-center justify-between text-[12px]"
                >
                  {t('history.source.all')}
                  {sourceFilter === null && <Check className="size-3 text-primary" />}
                </DropdownMenuItem>
                {spaceMembers.map(member => (
                  <DropdownMenuItem
                    key={member.peerId}
                    onClick={() => setSourceFilter(member.peerId)}
                    className="flex items-center justify-between gap-2 text-[12px]"
                  >
                    <span className="truncate">{member.deviceName}</span>
                    {sourceFilter === member.peerId && (
                      <Check className="size-3 text-primary shrink-0" />
                    )}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          )}

          {/* Time range */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                aria-label={t('history.timeRange.label')}
                className={cn(
                  'flex items-center gap-1.5 h-7 px-3 rounded-full text-[12px] font-medium whitespace-nowrap transition-all duration-150',
                  hasTimeFilter
                    ? 'bg-foreground/8 text-foreground'
                    : 'text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted/40'
                )}
              >
                <Clock className="size-3" />
                {t(`history.timeRange.${timeRange}`)}
                <ChevronDown className="size-3 opacity-50" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-36">
              {TIME_RANGES.map(range => (
                <DropdownMenuItem
                  key={range}
                  onClick={() => setTimeRange(range)}
                  className="flex items-center justify-between text-[12px]"
                >
                  {t(`history.timeRange.${range}`)}
                  {timeRange === range && <Check className="size-3 text-primary" />}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>

          {/* Search */}
          <div className="flex items-center gap-1.5 bg-muted/40 rounded-full px-3 h-7 w-48 focus-within:bg-muted/60 transition-colors">
            <Search className="size-3.5 text-muted-foreground/50 shrink-0" />
            <input
              ref={searchInputRef}
              type="text"
              autoCorrect="off"
              autoCapitalize="off"
              spellCheck={false}
              value={searchQuery}
              onChange={e => setSearchQuery(e.target.value)}
              onKeyDown={e => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  setSubmittedQuery(searchQuery.trim())
                } else if (e.key === 'Escape') {
                  e.preventDefault()
                  searchInputRef.current?.blur()
                }
              }}
              placeholder={t('history.searchPlaceholder')}
              className="flex-1 bg-transparent text-[12px] text-foreground placeholder:text-muted-foreground/50 outline-none min-w-0"
            />
          </div>
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
          <div className="flex px-2 pt-1 pb-4">
            {columns.map((colItems, colIdx) => (
              <div
                key={colIdx}
                className={cn(
                  'flex-1 min-w-0 flex flex-col',
                  colIdx > 0 && 'border-l-[3px] border-double border-border/20'
                )}
              >
                {colItems.map(item => {
                  const isNew = !seenIds.has(item.id)
                  return (
                    <m.div
                      key={item.id}
                      initial={isNew ? { opacity: 0, y: 16 } : false}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{ type: 'spring', stiffness: 400, damping: 30 }}
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
            ))}
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
