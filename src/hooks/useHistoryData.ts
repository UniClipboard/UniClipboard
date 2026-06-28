import { useEffect, useMemo, useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter, filterToContentTypes, filterToTags } from '@/api/clipboardItems'
import { type TimeRangePreset } from '@/api/daemon/search'
import { type LiveSearchQueryModel } from '@/hooks/liveSearchModel'
import { useLiveSearch } from '@/hooks/useLiveSearch'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import type { ClipboardFileItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { useAppSelector } from '@/store/hooks'
import { type PendingClipboardEntry } from '@/store/slices/clipboardSlice'

/** Live-list page size; the window grows by this as the user scrolls. */
const PAGE_SIZE = 100

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
// sent to the engine (debounced while typing, immediate on Enter). The list
// window is owned by `useLiveSearch` and collapses back to one page whenever the
// query model changes, so no `searchLimit` lives here anymore.

interface SearchState {
  activeFilter: Filter
  searchQuery: string
  submittedQuery: string
  timeRange: TimeRangePreset
  sourceFilter: string | null
}

type SearchAction =
  | { type: 'setContentFilter'; value: Filter }
  | { type: 'setSourceFilter'; value: string | null }
  | { type: 'setTimeRange'; value: TimeRangePreset }
  | { type: 'setQuery'; value: string }
  | { type: 'submitQuery'; value: string }

const INITIAL_STATE: SearchState = {
  activeFilter: Filter.All,
  searchQuery: '',
  submittedQuery: '',
  timeRange: 'all_time',
  sourceFilter: null,
}

function searchReducer(state: SearchState, action: SearchAction): SearchState {
  switch (action.type) {
    case 'setContentFilter':
      return { ...state, activeFilter: action.value }
    case 'setSourceFilter':
      return { ...state, sourceFilter: action.value }
    case 'setTimeRange':
      return { ...state, timeRange: action.value }
    case 'setQuery':
      return { ...state, searchQuery: action.value }
    case 'submitQuery':
      return { ...state, submittedQuery: action.value }
  }
}

/**
 * History data layer: owns the search/filter register and drives the unified
 * live browse/search list (`useLiveSearch`). Browse and search are one engine
 * path now — an empty query browses, any filter narrows — so there is no second
 * Redux-backed list and no mode switch; the `isSearchActive` flag only drives UI
 * affordances (empty-state copy, the result count), not a data source.
 */
export function useHistoryData() {
  const { t } = useTranslation()
  const [state, dispatch] = useReducer(searchReducer, INITIAL_STATE)

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

  // "Search active" = the user narrowed the view in any way (keyword, content
  // type, tag, time, or source). Drives empty-state copy and the result count
  // only; both browse and search read the same `useLiveSearch` list.
  const isSearchActive =
    state.submittedQuery.trim().length > 0 ||
    filterToContentTypes(state.activeFilter) !== undefined ||
    filterToTags(state.activeFilter) !== undefined ||
    state.timeRange !== 'all_time' ||
    state.sourceFilter !== null

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

  // ── Unified live browse/search list ───────────────────────────
  const model = useMemo<LiveSearchQueryModel>(
    () => ({
      query: state.submittedQuery.trim(),
      contentTypes: filterToContentTypes(state.activeFilter),
      tags: filterToTags(state.activeFilter),
      sourceDevices: state.sourceFilter ?? undefined,
      timeRange: state.timeRange,
    }),
    [state.submittedQuery, state.activeFilter, state.sourceFilter, state.timeRange]
  )

  const live = useLiveSearch({ model, pageSize: PAGE_SIZE })

  // Merge incoming-transfer placeholders (Redux overlay, still owned by the
  // event reducer) ahead of the real entries from the engine.
  const baseItems = useMemo<DisplayClipboardItem[]>(() => {
    // Pending placeholders only belong in the unfiltered browse view; a narrowed
    // query/filter set must show exactly what the engine returned, or unrelated
    // inbound file placeholders leak into results the backend would exclude.
    if (isSearchActive) return live.items
    const realIds = new Set(live.items.map(it => it.id))
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
    return [...pendingDisplayItems, ...live.items]
  }, [isSearchActive, live.items, pendingItems, deviceNameByPeerId, t])

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
    /** Match count for the current query (total history count while browsing). */
    browseCount: live.total ?? live.items.length,
    isSearchActive,
    searchLoading: live.isLoading,
    hasMore: live.hasMore,
    handleLoadMore: live.growWindow,
    /** Index availability for the current query: `degraded` while rebuilding. */
    indexState: live.state,
    /** Optimistically drop an entry after the user deletes it. */
    removeItem: live.removeItem,
  }
}
