import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { querySearch } from '@/api/daemon/search'
import type { HistoryLiveSnapshot } from '@/hooks/historySessionSnapshot'
import { shouldRefetchOnSearchStatus, type SearchStatusEventPayload } from '@/hooks/liveSearchModel'
import { liveModelToSearchParams, type LiveSearchQueryModel } from '@/hooks/liveSearchModel'
import { useClipboardEventStream } from '@/hooks/useClipboardEventStream'
import { useEncryptionSessionState } from '@/hooks/useEncryptionSessionState'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { searchResultToDisplayItem } from '@/lib/clipboard-transform'
import { daemonWs } from '@/lib/daemon-ws'
import {
  HISTORY_WINDOW_CAPACITY,
  HISTORY_WINDOW_BUFFER as BUFFER,
  HISTORY_WINDOW_IDLE_MS as IDLE_MS,
} from '@/lib/history-window-policy'
import { createLogger } from '@/lib/logger'
import type { SearchQueryFailure } from '@/lib/search-query-failure'

type RecordWindow = {
  offset: number
  items: DisplayClipboardItem[]
  modelKey: string
  reveal?: { id: string }
  targetIndex?: number
}

const EMPTY_ITEMS: DisplayClipboardItem[] = []
const log = createLogger('history-window')

function bufferedRange(start: number, end: number) {
  const visibleCount = end - start + 1
  const before = Math.min(
    BUFFER,
    Math.max(0, Math.floor((HISTORY_WINDOW_CAPACITY - visibleCount) / 2))
  )
  const offset = Math.max(0, start - before)
  return { offset, count: Math.min(HISTORY_WINDOW_CAPACITY, end - offset + BUFFER + 1) }
}

/** Owns only the records around the visible range; discarded pages are fetched again on return. */
export function useHistoryWindow(
  model: LiveSearchQueryModel,
  snapshot: HistoryLiveSnapshot | null = null
) {
  const { encryptionReady, isLocked } = useEncryptionSessionState()
  const modelKey = JSON.stringify(model)
  const [window, setWindow] = useState<RecordWindow>(() =>
    snapshot && JSON.stringify(snapshot.model) === JSON.stringify(model) && !model.query.trim()
      ? {
          offset: snapshot.offset ?? 0,
          items: snapshot.items.slice(0, HISTORY_WINDOW_CAPACITY),
          modelKey,
        }
      : { offset: 0, items: [], modelKey }
  )
  const [total, setTotal] = useState<number | null>(() =>
    snapshot && JSON.stringify(snapshot.model) === modelKey && !model.query.trim()
      ? snapshot.total
      : null
  )
  const [loading, setLoading] = useState(true)
  const [headRevision, setHeadRevision] = useState(0)
  const [failure, setFailure] = useState<SearchQueryFailure | null>(null)
  const [state, setState] = useState<'ready' | 'degraded'>('ready')
  const current = useRef(window)
  useLayoutEffect(() => {
    current.current = window
  }, [window])
  const firstQuery = useRef(true)
  const previousModel = useRef(model)
  const request = useRef<AbortController | null>(null)
  const refreshing = useRef(false)
  const requestedRange = useRef<{ offset: number; count: number; locating?: boolean } | null>(null)
  const idle = useRef<ReturnType<typeof setTimeout> | null>(null)
  const idleMode = useRef(false)
  const visible = useRef({ start: 0, end: 29 })
  const totalRef = useRef(total)
  useLayoutEffect(() => {
    totalRef.current = total
  }, [total])
  const load = useCallback(
    async (
      offset: number,
      count: number,
      refresh = false,
      revealIndex?: number,
      cursor?: { id: string; index: number; delta: number }
    ) => {
      // Superseding work inherits refresh ownership until an authoritative read completes.
      refresh = refresh || refreshing.current
      refreshing.current = refresh
      request.current?.abort()
      const controller = new AbortController()
      request.current = controller
      offset = Math.max(0, offset)
      count = Math.max(0, Math.min(HISTORY_WINDOW_CAPACITY, count))
      requestedRange.current = { offset, count, locating: cursor !== undefined }
      let end = offset + count
      const previous = current.current
      let kept = refresh
        ? []
        : previous.items.filter(
            (_, i) => previous.offset + i >= offset && previous.offset + i < end
          )
      let keptOffset = Math.max(offset, previous.offset)
      setLoading(true)
      setFailure(null)
      try {
        const readPage = async (start: number, stop: number) => {
          if (controller.signal.aborted) throw new DOMException('Aborted', 'AbortError')
          const response = (
            await querySearch(
              { ...liveModelToSearchParams(model), offset: start, limit: stop - start },
              controller.signal
            )
          ).data
          return {
            offset: start,
            items: response.items.map(searchResultToDisplayItem),
            total: response.total,
            state: response.state,
          }
        }
        if (cursor) {
          // An evicted cursor's last known rank may have changed without any
          // overlap between windows. Probe that rank, then scan bounded pages
          // only when necessary; never publish these probes as record windows.
          const first = Math.max(0, cursor.index - BUFFER)
          let page = await readPage(first, first + HISTORY_WINDOW_CAPACITY)
          let found = page.items.findIndex(item => item.id === cursor.id)
          for (let scan = 0; found < 0 && scan < page.total; scan += HISTORY_WINDOW_CAPACITY) {
            if (controller.signal.aborted) return
            if (scan === first) continue
            page = await readPage(scan, scan + HISTORY_WINDOW_CAPACITY)
            found = page.items.findIndex(item => item.id === cursor.id)
          }
          if (controller.signal.aborted || found < 0) return
          revealIndex = Math.max(
            0,
            Math.min(Math.max(0, page.total - 1), page.offset + found + cursor.delta)
          )
          offset = Math.max(0, revealIndex - BUFFER)
          end = offset + count
          kept = []
          keptOffset = offset
          refreshing.current = true
          requestedRange.current = { offset, count, locating: cursor !== undefined }
        }
        const parts: Array<{ offset: number; items: DisplayClipboardItem[] }> = []
        if (kept.length) parts.push({ offset: keptOffset, items: kept })
        const gaps = kept.length
          ? [
              [offset, keptOffset],
              [keptOffset + kept.length, end],
            ]
          : [[offset, end]]
        const pages = await Promise.all(
          gaps.filter(([start, stop]) => stop > start).map(([start, stop]) => readPage(start, stop))
        )
        let actualOffset = offset
        if (pages.length && offset >= pages[0].total) {
          actualOffset = Math.max(0, pages[0].total - count)
          parts.length = 0
          if (pages[0].total > 0) {
            pages.splice(0, pages.length, await readPage(actualOffset, actualOffset + count))
          }
        }
        if (controller.signal.aborted) return
        if (pages.length) {
          setTotal(pages[0].total)
          setState(pages[0].state)
        }
        parts.push(...pages)
        parts.sort((a, b) => a.offset - b.offset)
        const items = parts.flatMap(part => part.items).slice(0, HISTORY_WINDOW_CAPACITY)
        const target = revealIndex === undefined ? null : items[revealIndex - actualOffset]
        const next: RecordWindow = {
          modelKey,
          offset: actualOffset,
          items,
          reveal: target ? { id: target.id } : undefined,
          targetIndex: revealIndex,
        }
        current.current = next
        setWindow(next)
        return next
      } catch (error) {
        if (controller.signal.aborted) return
        if (
          error instanceof DaemonApiError &&
          error.code === DaemonErrorCode.ENCRYPTION_NOT_READY
        ) {
          setWindow({ offset: 0, items: [], modelKey })
          setTotal(null)
          return
        }
        const unavailable =
          error instanceof DaemonApiError && error.code === DaemonErrorCode.SERVICE_UNAVAILABLE
        const invalid =
          error instanceof DaemonApiError &&
          typeof error.details === 'object' &&
          error.details !== null &&
          'code' in error.details &&
          error.details.code === 'invalid_query'
        setFailure(invalid ? 'invalid' : unavailable ? 'unavailable' : 'failed')
        if (!invalid)
          log.warn(
            { code: error instanceof DaemonApiError ? error.code : 'UNKNOWN' },
            'History window query failed'
          )
        controller.abort()
      } finally {
        if (request.current === controller) {
          refreshing.current = false
          request.current = null
          requestedRange.current = null
          setLoading(false)
        }
      }
    },
    [model, modelKey]
  )
  useEffect(() => {
    request.current?.abort()
    if (previousModel.current !== model) {
      previousModel.current = model
      current.current = { offset: 0, items: [], modelKey }
      setWindow(current.current)
      setTotal(null)
    }
    if (!encryptionReady) {
      if (isLocked) {
        setWindow({ offset: 0, items: [], modelKey })
        setTotal(null)
      }
      return
    }
    idleMode.current = false
    visible.current = { start: 0, end: 29 }
    const offset = firstQuery.current ? current.current.offset : 0
    firstQuery.current = false
    void load(offset, HISTORY_WINDOW_CAPACITY, true)
    return () => {
      request.current?.abort()
      if (idle.current) clearTimeout(idle.current)
      idle.current = null
    }
  }, [encryptionReady, isLocked, load, model])
  const reportViewport = useCallback(
    (start: number, end: number, scrolling = false) => {
      if (scrolling) idleMode.current = false
      const absolute = { start: window.offset + start, end: window.offset + end }
      visible.current = absolute
      if (idle.current && scrolling) {
        clearTimeout(idle.current)
        idle.current = null
      }
      const { offset } = bufferedRange(absolute.start, absolute.end)
      const count = Math.min(
        HISTORY_WINDOW_CAPACITY,
        Math.max(0, (totalRef.current ?? Infinity) - offset)
      )
      const cached = current.current
      const pending = requestedRange.current
      const samePending =
        !pending?.locating && pending?.offset === offset && pending.count === count
      const superseding = scrolling && pending !== null && !samePending
      if (
        (!loading || scrolling) &&
        !idleMode.current &&
        (absolute.start < cached.offset + BUFFER ||
          absolute.end >= cached.offset + cached.items.length - BUFFER ||
          superseding)
      ) {
        if (
          !samePending &&
          (offset !== cached.offset || count > cached.items.length || superseding)
        )
          void load(offset, count)
      }
      if (idle.current) return
      idle.current = setTimeout(() => {
        idle.current = null
        if (requestedRange.current?.locating) return
        idleMode.current = true
        const range = visible.current
        const { offset: first, count: bufferedCount } = bufferedRange(range.start, range.end)
        const count = Math.min(bufferedCount, Math.max(0, (totalRef.current ?? Infinity) - first))
        if (current.current.offset !== first || current.current.items.length !== count)
          void load(first, count)
      }, IDLE_MS)
    },
    [load, loading, window.offset]
  )
  const navigate = useCallback(
    (direction: 'newer' | 'older') => {
      if (!encryptionReady || request.current || totalRef.current === null) return
      const offset = Math.max(
        0,
        Math.min(
          Math.max(0, totalRef.current - HISTORY_WINDOW_CAPACITY),
          current.current.offset + (direction === 'older' ? BUFFER : -BUFFER)
        )
      )
      if (offset === current.current.offset) return
      if (idle.current) clearTimeout(idle.current)
      idle.current = null
      idleMode.current = false
      void load(offset, HISTORY_WINDOW_CAPACITY)
    },
    [encryptionReady, load]
  )
  const refetch = useCallback(() => {
    void load(
      current.current.offset,
      Math.min(HISTORY_WINDOW_CAPACITY, Math.max(1, current.current.items.length)),
      true
    )
  }, [load])
  useEffect(() => {
    if (!encryptionReady) return
    const reconnect = daemonWs.onReconnect(refetch)
    const search = daemonWs.subscribe<SearchStatusEventPayload>(['search'], event => {
      if (shouldRefetchOnSearchStatus(event.payload, state)) refetch()
    })
    return () => {
      reconnect()
      search()
    }
  }, [encryptionReady, refetch, state])
  useClipboardEventStream({
    enabled: encryptionReady,
    onLocalItem: refetch,
    onLocalInvalidate: refetch,
    onRemoteInvalidate: refetch,
    onDeleted: refetch,
  })
  const ensureItem = useCallback(
    async (index: number) => {
      index = Math.max(0, Math.min(Math.max(0, (totalRef.current ?? 1) - 1), index))
      const cached = current.current
      const item = cached.items[index - cached.offset]
      if (item && !refreshing.current) {
        const next = { ...cached, reveal: { id: item.id } }
        current.current = next
        setWindow(next)
        return item
      }
      if (idle.current) clearTimeout(idle.current)
      idle.current = null
      idleMode.current = false
      const next = await load(Math.max(0, index - BUFFER), HISTORY_WINDOW_CAPACITY, false, index)
      return next?.items[index - next.offset] ?? null
    },
    [load]
  )
  const ensureNeighbor = useCallback(
    async (id: string, index: number, delta: number) => {
      if (idle.current) clearTimeout(idle.current)
      idle.current = null
      idleMode.current = false
      const next = await load(
        Math.max(0, index - BUFFER),
        HISTORY_WINDOW_CAPACITY,
        true,
        undefined,
        { id, index, delta }
      )
      if (!next || next.targetIndex === undefined) return null
      const item = next.items[next.targetIndex - next.offset]
      return item ? { item, index: next.targetIndex } : null
    },
    [load]
  )
  return {
    items: window.modelKey === modelKey ? window.items : EMPTY_ITEMS,
    resultsReady: window.modelKey === modelKey && total !== null,
    resultKey: modelKey,
    isAtTop: () => current.current.offset === 0 && visible.current.start === 0,
    ensureItem,
    ensureNeighbor,
    reveal: window.modelKey === modelKey ? window.reveal : undefined,
    headRevision,
    moveToTop: async () => {
      const next = await load(0, HISTORY_WINDOW_CAPACITY, true)
      if (next) {
        visible.current = { start: 0, end: Math.min(29, Math.max(0, next.items.length - 1)) }
        setHeadRevision(revision => revision + 1)
      }
    },
    offset: window.offset,
    total,
    isLoading: encryptionReady && loading,
    failure,
    state,
    reportViewport,
    navigate,
    refetch,
    hasMore: total !== null && window.offset + window.items.length < total,
    growWindow: () => {
      if (!loading) void load(window.offset + 10, HISTORY_WINDOW_CAPACITY)
    },
    removeItem: (id: string) => {
      setWindow(value => {
        const next = { ...value, items: value.items.filter(item => item.id !== id) }
        current.current = next
        return next
      })
    },
  }
}
