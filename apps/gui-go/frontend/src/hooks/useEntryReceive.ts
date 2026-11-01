import { useCallback, useEffect, useRef, useState } from 'react'
import { getEntryDetail } from '@/api/daemon/clipboard'
import { getEntryReceive, retryEntryReceive } from '@/api/file_transfer'
import { isFileContentType, parseFileItemsFromUriList } from '@/lib/clipboard-utils'
import { daemonWs } from '@/lib/daemon-ws'
import {
  isOlderReceiveSnapshot,
  receiveFileMappingKey,
  type ReceiveHistoryFiles,
} from '@/lib/receive-file-progress'
import { retryableItemIds, retryOutcomeMessageKey, type ReceiveSnapshot } from '@/lib/receive-items'
import { reportError } from '@/observability/errors'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  applyReceiveSnapshot,
  invalidateReceiveSnapshot,
  selectReceiveCancellationIncomplete,
  selectReceiveSnapshot,
} from '@/store/slices/fileTransferSlice'

/** What the last retry request told the user; `accepted` is information, not success. */
export interface RetryMessage {
  key: string
  tone: 'info' | 'warning' | 'error'
}

/**
 * Per-item receive results of one entry.
 *
 * The daemon snapshot is the only source of item results. Events (attempt state
 * and transfer progress) are invalidation signals that trigger a complete read. A reply is only
 * applied if it answers the latest request, and the store drops replies of
 * attempts that were already replaced.
 */
export function useEntryReceive(entryId: string | undefined) {
  const dispatch = useAppDispatch()
  const snapshot = useAppSelector(state =>
    entryId ? selectReceiveSnapshot(state, entryId) : undefined
  )
  const cancelIncomplete = useAppSelector(state =>
    entryId ? selectReceiveCancellationIncomplete(state, entryId) : false
  )
  const [retrying, setRetrying] = useState(false)
  const [retryMessage, setRetryMessage] = useState<RetryMessage | null>(null)
  const [metadata, setMetadata] = useState<{ key: string; files: ReceiveHistoryFiles } | null>(null)
  const refreshRef = useRef<() => Promise<void>>(async () => {})
  const retireRef = useRef<() => void>(() => {})

  useEffect(() => {
    if (!entryId) return
    let cancelled = false
    let generation = 0
    let dirty = false
    let running: Promise<void> | null = null
    let timer: ReturnType<typeof setTimeout> | null = null
    let accepted: ReceiveSnapshot | null = null
    let cached: { key: string; files: ReceiveHistoryFiles } | null = null
    const retire = () => {
      generation += 1
      cached = null
      accepted = null
      dispatch(invalidateReceiveSnapshot(entryId))
    }
    const refresh = (): Promise<void> => {
      dirty = true
      if (running) return running
      running = (async () => {
        while (dirty && !cancelled) {
          dirty = false
          const ticket = generation
          try {
            let next = await getEntryReceive(entryId)
            if (cancelled || ticket !== generation) continue
            if (!next) {
              dispatch(invalidateReceiveSnapshot(entryId))
              continue
            }
            if (accepted && isOlderReceiveSnapshot(next, accepted)) continue
            const key = receiveFileMappingKey(next)
            if (next.items.some(item => item.historyFileIndex != null) && cached?.key !== key) {
              const detail = await getEntryDetail(entryId)
              const confirmed = await getEntryReceive(entryId)
              if (cancelled || ticket !== generation) continue
              if (!confirmed || receiveFileMappingKey(confirmed) !== key) {
                dirty = true
                continue
              }
              if (accepted && isOlderReceiveSnapshot(confirmed, accepted)) continue
              next = confirmed
              if (detail?.mimeType && isFileContentType(detail.mimeType)) {
                cached = { key, files: parseFileItemsFromUriList(detail.content) }
                if (!cancelled && ticket === generation) setMetadata(cached)
              }
            }
            accepted = next
            dispatch(applyReceiveSnapshot(next))
          } catch (error) {
            if (!cancelled && ticket === generation)
              reportError(error, { command: 'getEntryReceive' })
          }
        }
      })().finally(() => {
        running = null
      })
      return running
    }
    const schedule = () => {
      if (timer) return
      timer = setTimeout(() => {
        timer = null
        void refresh()
      }, 100)
    }
    const reconnect = () => {
      retire()
      void refresh()
    }
    const visible = () => {
      if (document.visibilityState === 'visible') schedule()
    }
    refreshRef.current = refresh
    retireRef.current = retire
    retire()
    void refresh()
    const unsubscribe = daemonWs.subscribe(['file-transfer', 'clipboard', 'system'], event => {
      if (event.eventType === 'system.refresh_required') {
        reconnect()
        return
      }
      if (event.eventType === 'file-transfer.receive_progress_changed') {
        schedule()
        return
      }
      if (
        ![
          'file-transfer.progress',
          'file-transfer.status_changed',
          'clipboard.receive_attempt_state_changed',
          'clipboard.new_content',
        ].includes(event.eventType)
      )
        return
      const payload = event.payload as { entryId?: string } | null
      if (payload?.entryId === entryId) schedule()
    })
    const offReconnect = daemonWs.onReconnect(reconnect)
    document.addEventListener('visibilitychange', visible)
    return () => {
      cancelled = true
      generation += 1
      if (timer) clearTimeout(timer)
      unsubscribe()
      offReconnect()
      document.removeEventListener('visibilitychange', visible)
      if (refreshRef.current === refresh) {
        refreshRef.current = async () => {}
        retireRef.current = () => {}
      }
    }
  }, [dispatch, entryId])

  const retry = useCallback(async () => {
    if (!entryId || !snapshot || retrying) return
    const itemIds = retryableItemIds(snapshot)
    if (itemIds.length === 0) return
    retireRef.current()
    setRetrying(true)
    try {
      const result = await retryEntryReceive(entryId, snapshot.attemptId, itemIds)
      setRetryMessage({
        key: retryOutcomeMessageKey(result),
        tone: result.outcome === 'accepted' ? 'info' : 'warning',
      })
      // Whatever the outcome, the next snapshot (a new attempt when accepted) is the truth.
      retireRef.current()
      await refreshRef.current()
    } catch (error) {
      reportError(error, { command: 'retryEntryReceive' })
      setRetryMessage({ key: 'clipboard.receive.retry.failed', tone: 'error' })
    } finally {
      setRetrying(false)
    }
  }, [entryId, retrying, snapshot])

  const historyFiles =
    snapshot && metadata?.key === receiveFileMappingKey(snapshot) ? metadata.files : null
  return { snapshot, historyFiles, cancelIncomplete, retrying, retryMessage, retry }
}
