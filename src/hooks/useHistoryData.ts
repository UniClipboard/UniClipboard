import { useCallback, useEffect, useMemo, useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter, filterToContentTypes } from '@/api/clipboardItems'
import { type SearchResultDto, type TimeRangePreset } from '@/api/daemon/search'
import { useClipboardEvents } from '@/hooks/useClipboardEvents'
import { useClipboardSearch } from '@/hooks/useClipboardSearch'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import type { ClipboardFileItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { useAppSelector } from '@/store/hooks'
import { type PendingClipboardEntry } from '@/store/slices/clipboardSlice'

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

// ── Search/filter state ─────────────────────────────────────────
// `searchQuery` is the raw input value; `submittedQuery` is what was actually
// sent to the engine (debounced while typing, immediate on Enter). The search
// window (`searchLimit`) collapses back to one page on any filter/query change
// — handled inside the reducer so it never needs a chained state-update effect.

interface SearchState {
  activeFilter: Filter
  searchQuery: string
  submittedQuery: string
  timeRange: TimeRangePreset
  sourceFilter: string | null
  searchLimit: number
}

type SearchAction =
  | { type: 'setContentFilter'; value: Filter }
  | { type: 'setSourceFilter'; value: string | null }
  | { type: 'setTimeRange'; value: TimeRangePreset }
  | { type: 'setQuery'; value: string }
  | { type: 'submitQuery'; value: string }
  | { type: 'growWindow' }

const INITIAL_STATE: SearchState = {
  activeFilter: Filter.All,
  searchQuery: '',
  submittedQuery: '',
  timeRange: 'all_time',
  sourceFilter: null,
  searchLimit: SEARCH_PAGE_SIZE,
}

function searchReducer(state: SearchState, action: SearchAction): SearchState {
  switch (action.type) {
    case 'setContentFilter':
      return { ...state, activeFilter: action.value, searchLimit: SEARCH_PAGE_SIZE }
    case 'setSourceFilter':
      return { ...state, sourceFilter: action.value, searchLimit: SEARCH_PAGE_SIZE }
    case 'setTimeRange':
      return { ...state, timeRange: action.value, searchLimit: SEARCH_PAGE_SIZE }
    case 'setQuery':
      return { ...state, searchQuery: action.value }
    case 'submitQuery':
      return { ...state, submittedQuery: action.value, searchLimit: SEARCH_PAGE_SIZE }
    case 'growWindow':
      return { ...state, searchLimit: state.searchLimit + SEARCH_PAGE_SIZE }
  }
}

/**
 * History data layer: owns the search/filter register, runs the debounced search
 * engine query (or the browse list in its absence), and exposes the unified item
 * list plus the source-filter options. Keeping this out of the page component
 * collapses a dozen related useState calls into one reducer and lets the page
 * focus on interaction + layout.
 */
export function useHistoryData() {
  const { t } = useTranslation()
  const [state, dispatch] = useReducer(searchReducer, INITIAL_STATE)

  const items = useAppSelector(s => s.clipboard.items)
  const pendingItems = useAppSelector(s => s.clipboard.pendingItems)
  const spaceMembers = useAppSelector(s => s.devices.spaceMembers)
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
  const hasTypeFilter = state.activeFilter !== Filter.All && state.activeFilter !== Filter.Favorited
  const hasTimeFilter = state.timeRange !== 'all_time'
  const hasSourceFilter = state.sourceFilter !== null
  const isSearchActive =
    state.submittedQuery.trim().length > 0 || hasTypeFilter || hasTimeFilter || hasSourceFilter

  // Auto-submit while typing (debounced); clearing the input drops straight back
  // to browse mode. Enter bypasses the debounce via `actions.submitQuery`.
  useEffect(() => {
    const q = state.searchQuery.trim()
    if (!q) {
      dispatch({ type: 'submitQuery', value: '' })
      return
    }
    const timer = setTimeout(() => dispatch({ type: 'submitQuery', value: q }), 800)
    return () => clearTimeout(timer)
  }, [state.searchQuery])

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

  const {
    results: searchResults,
    isSearching: searchLoading,
    total: searchTotal,
  } = useClipboardSearch(
    {
      enabled: isSearchActive,
      query: state.submittedQuery.trim(),
      contentTypes: filterToContentTypes(state.activeFilter),
      sourceDevices: state.sourceFilter ?? undefined,
      timePreset: hasTimeFilter ? state.timeRange : undefined,
      limit: state.searchLimit,
    },
    mapSearchResult
  )

  const { hasMore, handleLoadMore } = useClipboardEvents(state.activeFilter)

  // In search mode show engine results; otherwise the browse (paginated) list.
  const baseItems = useMemo<DisplayClipboardItem[]>(
    () => (isSearchActive ? (searchResults ?? []) : displayItems),
    [isSearchActive, searchResults, displayItems]
  )

  const actions = useMemo(
    () => ({
      setContentFilter: (value: Filter) => dispatch({ type: 'setContentFilter', value }),
      setSourceFilter: (value: string | null) => dispatch({ type: 'setSourceFilter', value }),
      setTimeRange: (value: TimeRangePreset) => dispatch({ type: 'setTimeRange', value }),
      setQuery: (value: string) => dispatch({ type: 'setQuery', value }),
      submitQuery: (value: string) => dispatch({ type: 'submitQuery', value }),
    }),
    []
  )

  const growSearchWindow = useCallback(() => dispatch({ type: 'growWindow' }), [])

  return {
    filter: {
      activeFilter: state.activeFilter,
      searchQuery: state.searchQuery,
      submittedQuery: state.submittedQuery,
      timeRange: state.timeRange,
      sourceFilter: state.sourceFilter,
    },
    actions,
    sourceOptions,
    baseItems,
    /** Browse-list length (shown in the search bar even while searching). */
    browseCount: displayItems.length,
    isSearchActive,
    searchLoading,
    searchTotal,
    searchLoadedCount: searchResults?.length ?? 0,
    hasMore,
    handleLoadMore,
    growSearchWindow,
  }
}
