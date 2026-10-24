import { useCallback, useEffect, useRef, useState } from 'react'
import { getEntryReceive, retryEntryReceive } from '@/api/file_transfer'
import { retryableItemIds, retryOutcomeMessageKey } from '@/lib/receive-items'
import { reportError } from '@/observability/errors'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  applyReceiveSnapshot,
  entryStatusForAttemptState,
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
 * and transfer progress in the store) are invalidation signals: when they stop
 * agreeing with the stored snapshot, the snapshot is read again. A reply is only
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
  const currentAttempt = useAppSelector(state =>
    entryId ? state.fileTransfer.currentAttemptByEntry?.[entryId] : undefined
  )
  const entryStatus = useAppSelector(state =>
    entryId ? state.fileTransfer.entryStatusById[entryId]?.status : undefined
  )
  // Items whose bytes have fully arrived: a new one means saving is about to settle.
  const arrivedItems = useAppSelector(state => {
    if (!entryId) return 0
    const transferId = state.fileTransfer.entryTransferMap[entryId]
    const progress = transferId
      ? state.fileTransfer.activeTransfers[transferId]?.itemProgress
      : undefined
    if (!progress) return 0
    return Object.values(progress).filter(
      item => item.totalBytes !== null && item.bytesTransferred >= item.totalBytes
    ).length
  })

  const [retrying, setRetrying] = useState(false)
  const [retryMessage, setRetryMessage] = useState<RetryMessage | null>(null)
  const latestRequest = useRef(0)

  const refresh = useCallback(async () => {
    if (!entryId) return
    const request = ++latestRequest.current
    try {
      const next = await getEntryReceive(entryId)
      if (request !== latestRequest.current || !next) return
      dispatch(applyReceiveSnapshot(next))
    } catch (error) {
      reportError(error, { command: 'getEntryReceive' })
    }
  }, [dispatch, entryId])

  const inSync =
    !!snapshot &&
    snapshot.attemptId === currentAttempt &&
    // `cancelling` / `failing` leave the entry status alone, so they never disagree.
    (entryStatusForAttemptState(snapshot.state) ?? entryStatus) === entryStatus
  const lastRead = useRef<{ entryId: string; arrivedItems: number } | null>(null)

  useEffect(() => {
    if (!entryId) return
    const previous = lastRead.current
    lastRead.current = { entryId, arrivedItems }
    const firstRead = previous?.entryId !== entryId
    const itemArrived = previous !== null && !firstRead && previous.arrivedItems !== arrivedItems
    // Read on first sight, when an item's bytes arrived, and whenever events and
    // the stored snapshot disagree. Applying a snapshot restores agreement, so a
    // read never triggers another read by itself.
    if (firstRead || itemArrived || !inSync) void refresh()
  }, [arrivedItems, entryId, inSync, refresh])

  const retry = useCallback(async () => {
    if (!entryId || !snapshot || retrying) return
    const itemIds = retryableItemIds(snapshot)
    if (itemIds.length === 0) return
    setRetrying(true)
    try {
      const result = await retryEntryReceive(entryId, snapshot.attemptId, itemIds)
      setRetryMessage({
        key: retryOutcomeMessageKey(result),
        tone: result.outcome === 'accepted' ? 'info' : 'warning',
      })
      // Whatever the outcome, the next snapshot (a new attempt when accepted) is the truth.
      await refresh()
    } catch (error) {
      reportError(error, { command: 'retryEntryReceive' })
      setRetryMessage({ key: 'clipboard.receive.retry.failed', tone: 'error' })
    } finally {
      setRetrying(false)
    }
  }, [entryId, refresh, retrying, snapshot])

  return { snapshot, cancelIncomplete, retrying, retryMessage, retry }
}
