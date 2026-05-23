import { useEffect } from 'react'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import { removePendingEntry } from '@/store/slices/clipboardSlice'
import {
  markTransferCancelled,
  markTransferCompleted,
  markTransferFailed,
  cancelClipboardWrite,
  linkTransferToEntry,
  normalizeCancelReason,
  setEntryTransferStatus,
  updateTransferProgress,
} from '@/store/slices/fileTransferSlice'

const log = createLogger('use-transfer-progress')
const transferProgressDebugEnabled = import.meta.env.DEV

interface FileTransferStatusEvent {
  transferId: string
  entryId: string
  status: string
  reason?: string | null
}

interface FileTransferProgressEvent {
  transferId: string
  entryId?: string | null
  peerId: string
  direction: 'Sending' | 'Receiving'
  bytesTransferred: number
  totalBytes?: number | null
}

interface DaemonTransferEvent {
  eventType: string
  payload: unknown
  ts: number
}

/**
 * Hook that listens to file-transfer and clipboard events from the daemon WS,
 * dispatching durable status changes to the Redux fileTransfer slice.
 *
 * - `file-transfer.status_changed` events update entry transfer status.
 * - `clipboard.new_content` events cancel pending clipboard auto-writes.
 *
 * Call once in a top-level component (e.g. ClipboardContent) to activate.
 */
export function useTransferProgress(): void {
  const dispatch = useAppDispatch()

  useEffect(() => {
    let cancelled = false
    const warnedMissingEntryLinkage = new Set<string>()

    const handler = (event: DaemonTransferEvent) => {
      if (cancelled) return

      if (event.eventType === 'clipboard.new_content') {
        dispatch(cancelClipboardWrite())
        return
      }

      if (event.eventType === 'file-transfer.status_changed') {
        const payload = event.payload as FileTransferStatusEvent
        const { entryId, status, reason } = payload
        if (transferProgressDebugEnabled) {
          log.debug(
            {
              transferId: payload.transferId,
              entryId,
              status,
              reason: reason ?? null,
            },
            'file transfer status changed'
          )
        }
        dispatch(linkTransferToEntry({ transferId: payload.transferId, entryId }))
        const validStatuses = [
          'pending',
          'transferring',
          'completed',
          'failed',
          'cancelled',
        ] as const
        if (validStatuses.includes(status as (typeof validStatuses)[number])) {
          dispatch(
            setEntryTransferStatus({
              entryId,
              status: status as (typeof validStatuses)[number],
              reason: reason ?? null,
            })
          )
        }

        if (status === 'failed') {
          dispatch(
            markTransferFailed({
              transferId: payload.transferId,
              error: reason ?? undefined,
            })
          )
        } else if (status === 'cancelled') {
          // 兜底清 placeholder:apply_inbound 的 partial 路径正常会再发
          // `clipboard.new_content` 触发 removePendingEntry,但 status_changed
          // 帧通常先到(它是 cancel arm 在撕 connection 之前 await 推过来的,
          // 而 NewContent 要等 capture 落库)。早一步清掉 pendingItems 能避
          // 免"取消后 placeholder 仍然置顶几秒"的视觉残留。幂等:
          // clipboard.new_content 到达时再 remove 一次没副作用。
          dispatch(removePendingEntry(entryId))
          dispatch(
            markTransferCancelled({
              transferId: payload.transferId,
              reason: normalizeCancelReason(reason),
            })
          )
        } else if (status === 'completed') {
          dispatch(markTransferCompleted({ transferId: payload.transferId }))
        }
      }

      if (event.eventType === 'file-transfer.progress') {
        const payload = event.payload as FileTransferProgressEvent
        if (transferProgressDebugEnabled) {
          log.debug(
            {
              transferId: payload.transferId,
              entryId: payload.entryId ?? null,
              peerId: payload.peerId,
              direction: payload.direction,
              bytesTransferred: payload.bytesTransferred,
              totalBytes: payload.totalBytes ?? null,
            },
            'file transfer progress'
          )
        }
        dispatch(
          updateTransferProgress({
            transferId: payload.transferId,
            entryId: payload.entryId ?? null,
            peerId: payload.peerId,
            direction: payload.direction,
            bytesTransferred: payload.bytesTransferred,
            totalBytes: payload.totalBytes ?? null,
            eventTs: event.ts,
          })
        )

        if (payload.entryId) {
          if (transferProgressDebugEnabled) {
            log.debug(
              {
                transferId: payload.transferId,
                entryId: payload.entryId,
              },
              'linked transfer to entry from progress event'
            )
          }
          dispatch(
            linkTransferToEntry({ transferId: payload.transferId, entryId: payload.entryId })
          )
        } else {
          if (!warnedMissingEntryLinkage.has(payload.transferId)) {
            warnedMissingEntryLinkage.add(payload.transferId)
            log.warn(
              {
                transferId: payload.transferId,
                peerId: payload.peerId,
                direction: payload.direction,
              },
              'progress event missing entry linkage'
            )
          }
        }
      }
    }

    const unsubscribe = daemonWs.subscribe(['file-transfer', 'clipboard'], handler)

    return () => {
      cancelled = true
      unsubscribe()
    }
  }, [dispatch])
}
