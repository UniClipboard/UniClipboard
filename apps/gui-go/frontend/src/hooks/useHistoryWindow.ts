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
  const [window, setWindow] = useState<{ offset: number; items: DisplayClipboardItem[] }>(() =>
    snapshot && JSON.stringify(snapshot.model) === JSON.stringify(model) && !model.query.trim()
      ? { offset: snapshot.offset ?? 0, items: snapshot.items.slice(0, HISTORY_WINDOW_CAPACITY) }
      : { offset: 0, items: [] }
  )
  const [total, setTotal] = useState<number | null>(null)
  const [loading, setLoading] = useState(true)
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
  const idle = useRef<ReturnType<typeof setTimeout> | null>(null)
  const idleMode = useRef(false)
  const visible = useRef({ start: 0, end: 29 })
  const totalRef = useRef(total)
  useLayoutEffect(() => {
    totalRef.current = total
  }, [total])
  const load = useCallback(
    async (offset: number, count: number, refresh = false) => {
      // Superseding work inherits refresh ownership until an authoritative read completes.
      refresh = refresh || refreshing.current
      refreshing.current = refresh
      request.current?.abort()
      const controller = new AbortController()
      request.current = controller
      const end = offset + Math.min(HISTORY_WINDOW_CAPACITY, count)
      const previous = current.current
      const kept = refresh
        ? []
        : previous.items.filter(
            (_, i) => previous.offset + i >= offset && previous.offset + i < end
          )
      const keptOffset = Math.max(offset, previous.offset)
      const retained = { offset: kept.length ? keptOffset : offset, items: kept }
      if (!refresh) {
        current.current = retained
        setWindow(retained)
      }
      setLoading(true)
      setFailure(null)
      try {
        const parts: Array<{ offset: number; items: DisplayClipboardItem[] }> = []
        if (kept.length) parts.push({ offset: keptOffset, items: kept })
        const gaps = kept.length
          ? [
              [offset, keptOffset],
              [keptOffset + kept.length, end],
            ]
          : [[offset, end]]
        const readPage = async (start: number, stop: number) => {
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
        const next = {
          offset: actualOffset,
          items: parts.flatMap(part => part.items).slice(0, HISTORY_WINDOW_CAPACITY),
        }
        current.current = next
        setWindow(next)
      } catch (error) {
        if (controller.signal.aborted) return
        if (
          error instanceof DaemonApiError &&
          error.code === DaemonErrorCode.ENCRYPTION_NOT_READY
        ) {
          setWindow({ offset: 0, items: [] })
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
          setLoading(false)
        }
      }
    },
    [model]
  )
  useEffect(() => {
    request.current?.abort()
    if (previousModel.current !== model) {
      previousModel.current = model
      current.current = { offset: 0, items: [] }
      setWindow(current.current)
    }
    if (!encryptionReady) {
      if (isLocked) {
        setWindow({ offset: 0, items: [] })
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
      if (
        !loading &&
        !idleMode.current &&
        (absolute.start < cached.offset + BUFFER ||
          absolute.end >= cached.offset + cached.items.length - BUFFER)
      ) {
        if (offset !== cached.offset || count > cached.items.length) void load(offset, count)
      }
      if (idle.current) return
      idle.current = setTimeout(() => {
        idle.current = null
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
  return {
    items: window.items,
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
    removeItem: (id: string) =>
      setWindow(value => ({ ...value, items: value.items.filter(item => item.id !== id) })),
  }
}
