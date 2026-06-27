import { useCallback, useEffect, useRef, useState } from 'react'
import { querySearch } from '@/api/daemon/search'
import { useClipboardEventStream } from '@/hooks/useClipboardEventStream'
import { useEncryptionSessionState } from '@/hooks/useEncryptionSessionState'
import type { ClipboardEntry, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { searchResultToDisplayItem } from '@/lib/clipboard-transform'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import {
  canPatchLive,
  matchesFilter,
  patchLiveItem,
  prependLiveItem,
  removeLiveItem,
  shouldRefetchOnSearchStatus,
  type LiveSearchQueryModel,
  type SearchStatusEventPayload,
} from './liveSearchModel'

const log = createLogger('use-live-search')

/** Default window size; the list grows by this as the user scrolls. */
const DEFAULT_PAGE_SIZE = 100

export interface UseLiveSearchOptions {
  /** When false the hook idles: empty list, no request, no subscription. */
  enabled?: boolean
  /** The resolved query/filter model; an empty `query` with no filters browses. */
  model: LiveSearchQueryModel
  /** Page size; {@link UseLiveSearchResult.growWindow} grows the window by this. */
  pageSize?: number
}

export interface UseLiveSearchResult {
  items: DisplayClipboardItem[]
  isLoading: boolean
  /** Engine-reported total match count, or null before the first response. */
  total: number | null
  hasMore: boolean
  /**
   * `'degraded'` when a filter-less browse was served from the main store while
   * the index rebuilds (§4.7); `'ready'` otherwise.
   */
  state: 'ready' | 'degraded'
  /** Grow the window by one page (infinite scroll). */
  growWindow: () => void
  /** Re-issue the current query (freshness on demand; e.g. on panel re-open). */
  refetch: () => void
  /** Optimistically drop an entry (user delete) before the next refetch. */
  removeItem: (id: string) => void
  /** Optimistically patch an entry in place (favorite toggle, payload lost). */
  patchItem: (id: string, patch: Partial<DisplayClipboardItem>) => void
}

/**
 * Unified live browse/search list (Phase 3B).
 *
 * Owns a single `DisplayClipboardItem[]` seeded by a base `querySearch` (an
 * empty query browses) and kept live by the clipboard WebSocket: a new entry is
 * either slotted in client-side (when {@link canPatchLive} + {@link matchesFilter}
 * pass) or triggers a refetch of the current window (§4.8 fallback). User
 * actions apply optimistic edits via {@link UseLiveSearchResult.removeItem} /
 * {@link UseLiveSearchResult.patchItem}.
 *
 * Replaces the old split of Redux-backed browse (`useClipboardEvents`) plus a
 * separate search executor: browse and search are now one path, and the index
 * `state` (ready/degraded) is surfaced honestly instead of the invented
 * `notReady` flag.
 */
export function useLiveSearch(options: UseLiveSearchOptions): UseLiveSearchResult {
  const { enabled = true, model, pageSize = DEFAULT_PAGE_SIZE } = options
  const { query, contentTypes, tags, sourceDevices, extensions, timeRange } = model
  const { encryptionReady } = useEncryptionSessionState()

  const [items, setItems] = useState<DisplayClipboardItem[]>([])
  // Start loading when enabled so the first paint shows a spinner, not a
  // flash of the empty state before the base query resolves.
  const [isLoading, setIsLoading] = useState(enabled)
  const [total, setTotal] = useState<number | null>(null)
  const [hasMore, setHasMore] = useState(false)
  const [state, setState] = useState<'ready' | 'degraded'>('ready')
  const [limit, setLimit] = useState(pageSize)
  const [refetchNonce, setRefetchNonce] = useState(0)

  const abortRef = useRef<AbortController | null>(null)

  // Collapse the window back to one page whenever the query model changes.
  useEffect(() => {
    setLimit(pageSize)
  }, [query, contentTypes, tags, sourceDevices, extensions, timeRange, pageSize])

  // Base query: (re)seed the list from the engine on model/window/refetch change.
  // Re-runs on `encryptionReady` too, so unlocking refetches (a keyword search is
  // rejected while locked and is skipped until then).
  useEffect(() => {
    abortRef.current?.abort()
    // A keyword search is rejected (423) while the session is locked; skip it and
    // show nothing until unlock. Filter-less/filter-only browse stays allowed
    // while locked (it may come back degraded), so it still runs.
    const keywordWhileLocked = query.trim().length > 0 && !encryptionReady
    if (!enabled || keywordWhileLocked) {
      setItems([])
      setTotal(null)
      setHasMore(false)
      setState('ready')
      setIsLoading(false)
      return
    }

    const controller = new AbortController()
    abortRef.current = controller
    setIsLoading(true)

    querySearch(
      {
        query,
        contentTypes,
        tags,
        extensions,
        sourceDevices,
        timePreset: timeRange && timeRange !== 'all_time' ? timeRange : undefined,
        limit,
      },
      controller.signal
    )
      .then(response => {
        if (controller.signal.aborted) return
        setItems(response.data.items.map(searchResultToDisplayItem))
        setTotal(response.data.total)
        setHasMore(response.data.hasMore)
        setState(response.data.state)
        setIsLoading(false)
      })
      .catch(err => {
        if (controller.signal.aborted) return
        if (err instanceof DOMException && err.name === 'AbortError') return
        log.error({ err }, 'Live search query failed')
        setItems([])
        setTotal(0)
        setHasMore(false)
        setState('ready')
        setIsLoading(false)
      })

    return () => controller.abort()
  }, [
    enabled,
    encryptionReady,
    query,
    contentTypes,
    tags,
    sourceDevices,
    extensions,
    timeRange,
    limit,
    refetchNonce,
  ])

  // Realtime: a new local entry is slotted in (when the current filters are
  // client-judgeable) or refetched; remote changes always refetch.
  const onLocalItem = useCallback(
    (entry: ClipboardEntry) => {
      const current: LiveSearchQueryModel = {
        query,
        contentTypes,
        tags,
        sourceDevices,
        extensions,
        timeRange,
      }
      if (!canPatchLive(current)) {
        setRefetchNonce(n => n + 1)
        return
      }
      const display: DisplayClipboardItem = {
        id: entry.id,
        type: entry.type,
        content: entry.content,
        activeTime: entry.activeTime,
        isFavorited: entry.isFavorited,
        isUnavailable: entry.isUnavailable,
      }
      if (!matchesFilter(display, current)) return
      setItems(prev => prependLiveItem(prev, display))
    },
    [query, contentTypes, tags, sourceDevices, extensions, timeRange]
  )

  const onRemoteInvalidate = useCallback(() => setRefetchNonce(n => n + 1), [])
  const onDeleted = useCallback((id: string) => setItems(prev => removeLiveItem(prev, id)), [])

  useClipboardEventStream({
    enabled: enabled && encryptionReady,
    onLocalItem,
    onRemoteInvalidate,
    onDeleted,
  })

  // The degraded banner is driven by the last query's `state`. While the index
  // rebuilds, a filter-less browse is served degraded (§4.7) and new local
  // entries are slotted in client-side without a refetch — so nothing re-queries
  // when the rebuild finishes, and the banner would otherwise persist forever.
  // Track the latest `state` in a ref so the WS handler can read it without
  // re-subscribing on every query.
  const stateRef = useRef(state)
  useEffect(() => {
    stateRef.current = state
  }, [state])

  // Subscribe to the search-index status stream and, once the index reports
  // `ready` again while we are showing the degraded view, refetch the current
  // window so it upgrades to the index-backed result (clearing the banner and
  // restoring filter/keyword search).
  useEffect(() => {
    if (!enabled) return
    return daemonWs.subscribe<SearchStatusEventPayload>(['search'], event => {
      if (shouldRefetchOnSearchStatus(event.payload, stateRef.current)) {
        setRefetchNonce(n => n + 1)
      }
    })
  }, [enabled])

  // D8 resync: the WS auto-resubscribes topics after a reconnect, but events
  // missed during the outage are not replayed. Re-issue the query so the window
  // reconciles to a fresh snapshot (querySearch is idempotent).
  useEffect(() => {
    if (!enabled) return
    return daemonWs.onReconnect(() => {
      if (encryptionReady) setRefetchNonce(n => n + 1)
    })
  }, [enabled, encryptionReady])

  const growWindow = useCallback(() => setLimit(value => value + pageSize), [pageSize])
  const refetch = useCallback(() => setRefetchNonce(n => n + 1), [])
  const removeItem = useCallback((id: string) => setItems(prev => removeLiveItem(prev, id)), [])
  const patchItem = useCallback(
    (id: string, patch: Partial<DisplayClipboardItem>) =>
      setItems(prev => patchLiveItem(prev, id, patch)),
    []
  )

  return { items, isLoading, total, hasMore, state, growWindow, refetch, removeItem, patchItem }
}
