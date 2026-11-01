import { useEffect, useMemo, useReducer, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter, filterToContentTypes, filterToTags } from '@/api/clipboardItems'
import { type TimeRangePreset } from '@/api/daemon/search'
import {
  readHistorySessionSnapshot,
  writeHistorySessionSnapshot,
  type HistoryLiveSnapshot,
} from '@/hooks/historySessionSnapshot'
import { buildLiveSearchModel, type LiveSearchQueryModel } from '@/hooks/liveSearchModel'
import { useHistorySourceOptions } from '@/hooks/useHistorySourceOptions'
import { useHistoryWindow } from '@/hooks/useHistoryWindow'
import { useHistoryWindowImages } from '@/hooks/useHistoryWindowImages'
import type { ClipboardFileItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { clipboardPreviewCache } from '@/lib/clipboard-preview-cache'
import { HISTORY_WINDOW_CAPACITY } from '@/lib/history-window-policy'
import { useAppSelector } from '@/store/hooks'
import { type PendingClipboardEntry } from '@/store/slices/clipboardSlice'

/** Maximum number of records retained by the history window. */
const PAGE_SIZE = HISTORY_WINDOW_CAPACITY

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
// window is owned by `useHistoryWindow` and resets whenever the
// query model changes, so no `searchLimit` lives here anymore.

interface SearchState {
  activeFilter: Filter
  searchQuery: string
  submittedQuery: string
  tagFilter: string | null
  timeRange: TimeRangePreset
  sourceFilter: string | null
  extensionFilter: string | null
}

type SearchAction =
  | { type: 'setContentFilter'; value: Filter }
  | { type: 'setTagFilter'; value: string | null }
  | { type: 'setSourceFilter'; value: string | null }
  | { type: 'setExtensionFilter'; value: string | null }
  | { type: 'setTimeRange'; value: TimeRangePreset }
  | { type: 'setQuery'; value: string }
  | { type: 'submitQuery'; value: string }

const INITIAL_STATE: SearchState = {
  activeFilter: Filter.All,
  searchQuery: '',
  submittedQuery: '',
  tagFilter: null,
  timeRange: 'all_time',
  sourceFilter: null,
  extensionFilter: null,
}

function getInitialSearchState(): SearchState {
  const snapshot = readHistorySessionSnapshot()?.searchState
  return snapshot ? { ...INITIAL_STATE, ...snapshot } : INITIAL_STATE
}

function searchReducer(state: SearchState, action: SearchAction): SearchState {
  switch (action.type) {
    case 'setContentFilter':
      return { ...state, activeFilter: action.value }
    case 'setTagFilter':
      return { ...state, tagFilter: action.value }
    case 'setSourceFilter':
      return { ...state, sourceFilter: action.value }
    case 'setExtensionFilter':
      return { ...state, extensionFilter: action.value }
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
export function useHistoryData(selectedId: string | null = null) {
  const { t } = useTranslation()
  const [state, dispatch] = useReducer(searchReducer, undefined, getInitialSearchState)

  const pendingItems = useAppSelector(s => s.clipboard.pendingItems)
  const spaceMembers = useAppSelector(s => s.devices.spaceMembers)
  const sourceOptions = useHistorySourceOptions()
  const localDevice = useAppSelector(s => s.devices.localDevice)

  // Origin id -> device name for the list rows' meta line: this device, paired
  // peers, and mobile-sync devices (the same roster the `from:` filter offers).
  const sourceDeviceNames = useMemo(() => {
    const map: Record<string, string> = {}
    for (const option of sourceOptions) map[option.id] = option.name
    if (localDevice) map[localDevice.peerId] = localDevice.deviceName
    return map
  }, [sourceOptions, localDevice])

  const deviceNameByPeerId = useMemo(() => {
    const map: Record<string, string> = {}
    for (const m of spaceMembers) map[m.peerId] = m.deviceName
    return map
  }, [spaceMembers])

  // "Search active" = the user narrowed the view in any way (keyword, content
  // type, tag, time, or source). Drives empty-state copy and the result count
  // only; both browse and search read the same `useLiveSearch` list.
  const isSearchActive =
    state.submittedQuery.trim().length > 0 ||
    filterToContentTypes(state.activeFilter) !== undefined ||
    filterToTags(state.activeFilter) !== undefined ||
    state.tagFilter !== null ||
    state.timeRange !== 'all_time' ||
    state.sourceFilter !== null ||
    state.extensionFilter !== null

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
    () =>
      buildLiveSearchModel({
        query: state.submittedQuery.trim(),
        activeFilter: state.activeFilter,
        tagFilter: state.tagFilter,
        sourceFilter: state.sourceFilter,
        extensionFilter: state.extensionFilter,
        timeRange: state.timeRange,
      }),
    [
      state.submittedQuery,
      state.activeFilter,
      state.tagFilter,
      state.sourceFilter,
      state.extensionFilter,
      state.timeRange,
    ]
  )

  const live = useHistoryWindow(model, readHistorySessionSnapshot()?.live ?? null)
  const previousSnapshot = readHistorySessionSnapshot()

  // Merge incoming-transfer placeholders (Redux overlay, still owned by the
  // event reducer) ahead of the real entries from the engine.
  const pendingDisplayItems = useMemo<DisplayClipboardItem[]>(() => {
    if (isSearchActive) return []
    const realIds = new Set(live.items.map(it => it.id))
    return pendingItems
      .flatMap(p =>
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
      .slice(0, PAGE_SIZE)
  }, [isSearchActive, live.items, pendingItems, deviceNameByPeerId, t])
  const pendingCount = pendingDisplayItems.length
  const baseItems = useMemo(
    () =>
      live.offset === 0 && pendingCount
        ? [...pendingDisplayItems, ...live.items].slice(0, PAGE_SIZE)
        : live.items,
    [live.offset, live.items, pendingDisplayItems, pendingCount]
  )
  useHistoryWindowImages(baseItems)
  useEffect(() => {
    clipboardPreviewCache.retain(new Set(baseItems.map(item => item.id)), selectedId)
  }, [baseItems, selectedId])
  const visibleStart = useRef(0)

  const actions = useMemo(
    () => ({
      setContentFilter: (value: Filter) => dispatch({ type: 'setContentFilter', value }),
      setTagFilter: (value: string | null) => dispatch({ type: 'setTagFilter', value }),
      setSourceFilter: (value: string | null) => dispatch({ type: 'setSourceFilter', value }),
      setExtensionFilter: (value: string | null) => dispatch({ type: 'setExtensionFilter', value }),
      setTimeRange: (value: TimeRangePreset) => dispatch({ type: 'setTimeRange', value }),
      setQuery: (value: string) => dispatch({ type: 'setQuery', value }),
      submitQuery: (value: string) => dispatch({ type: 'submitQuery', value }),
    }),
    []
  )

  const liveSnapshot = useMemo<HistoryLiveSnapshot>(
    () => ({
      model,
      offset: live.offset,
      items: live.items,
      total: live.total,
      hasMore: live.hasMore,
      state: live.state,
    }),
    [model, live.offset, live.items, live.total, live.hasMore, live.state]
  )

  useEffect(() => {
    writeHistorySessionSnapshot({
      searchState: state,
      live: liveSnapshot,
      selectedId,
      selectedItem: previousSnapshot?.selectedItem ?? null,
      selectedIndex: previousSnapshot?.selectedIndex ?? null,
      seenIds: previousSnapshot?.seenIds ?? [],
      scrollState: previousSnapshot?.scrollState ?? null,
    })
  }, [live.items.length, liveSnapshot, previousSnapshot, state, selectedId])

  return {
    filter: {
      activeFilter: state.activeFilter,
      searchQuery: state.searchQuery,
      submittedQuery: state.submittedQuery,
      tagFilter: state.tagFilter,
      timeRange: state.timeRange,
      sourceFilter: state.sourceFilter,
      extensionFilter: state.extensionFilter,
    },
    actions,
    sourceOptions,
    sourceDeviceNames,
    baseItems,
    liveSnapshot,
    /** Match count for the current query (total history count while browsing). */
    browseCount: live.total ?? live.items.length,
    /** Why the list query failed (`null` when it did not); a failure is not an empty history. */
    queryFailure: live.failure,
    isSearchActive,
    searchLoading: live.isLoading,
    hasMore: live.hasMore,
    handleLoadMore: live.growWindow,
    resultKey: live.resultKey,
    isAtTop: () => live.isAtTop() && visibleStart.current === 0,
    resultsReady: live.resultsReady,
    ensureItem: async (index: number) => {
      if (index < pendingCount) {
        if (live.offset > 0) await live.moveToTop()
        return pendingDisplayItems[Math.max(0, index)] ?? null
      }
      return live.ensureItem(index - pendingCount)
    },
    ensureNeighbor: async (id: string, index: number, delta: number) => {
      const pendingIndex = pendingDisplayItems.findIndex(item => item.id === id)
      if (pendingIndex >= 0) {
        const target = Math.max(
          0,
          Math.min(
            Math.max(0, (live.total ?? live.items.length) + pendingCount - 1),
            pendingIndex + delta
          )
        )
        const item =
          target < pendingCount
            ? pendingDisplayItems[target]
            : await live.ensureItem(target - pendingCount)
        if (target < pendingCount && live.offset > 0) await live.moveToTop()
        return item ? { item, index: target } : null
      }
      const result = await live.ensureNeighbor(id, Math.max(0, index - pendingCount), delta)
      return result ? { ...result, index: result.index + pendingCount } : null
    },
    moveToTop: live.moveToTop,
    viewport: {
      scrollToTop: live.headRevision,
      reveal: live.reveal,
      total: Math.max(
        baseItems.length + live.offset + (live.offset > 0 ? pendingCount : 0),
        (live.total ?? live.items.length) + pendingCount
      ),
      offset: live.offset > 0 ? live.offset + pendingCount : 0,
      onVisibleRange: (start: number, end: number, scrolling?: boolean) => {
        visibleStart.current = start
        const head = live.offset === 0 ? pendingCount : 0
        live.reportViewport(Math.max(0, start - head), Math.max(0, end - head), scrolling)
      },
      onNavigate: live.navigate,
    },
    /** Index availability for the current query: `degraded` while rebuilding. */
    indexState: live.state,
    /** Optimistically drop an entry after the user deletes it. */
    removeItem: live.removeItem,
    /** Re-query the current window, e.g. after its tags changed. */
    refetch: live.refetch,
  }
}
