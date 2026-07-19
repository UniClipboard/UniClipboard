import { useEffect } from 'react'
import { listEntryReceives } from '@/api/file_transfer'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import { addPendingEntry } from '@/store/slices/clipboardSlice'
import { hydrateReceiveAttempts } from '@/store/slices/fileTransferSlice'
import { createClipboardEventReducer } from './clipboardEventReducer'
import type { ClipboardRealtimeEvent } from './clipboardEventReducer'

const log = createLogger('use-transfer-progress')
const transferProgressDebugEnabled = import.meta.env.DEV

/**
 * Hook that listens to file-transfer events from the daemon WS and forwards
 * their state actions to Redux.
 *
 * - `file-transfer.status_changed` events update entry transfer status.
 * - `file-transfer.progress` events update live progress.
 *
 * Call once in a top-level component (e.g. ClipboardContent) to activate.
 */
export function useTransferProgress(): void {
  const dispatch = useAppDispatch()

  useEffect(() => {
    let cancelled = false
    const reducer = createClipboardEventReducer({ now: () => Date.now(), throttleMs: 300 })

    void listEntryReceives()
      .then(receives => {
        if (cancelled) return
        dispatch(hydrateReceiveAttempts(receives))
        for (const receive of receives) {
          dispatch(
            addPendingEntry({
              entryId: receive.entryId,
              attemptId: receive.attemptId,
              fromDevice: '',
              totalBytes: receive.totalBytes || null,
              filenames: [],
              createdAt: Date.now(),
            })
          )
        }
      })
      .catch(error => log.warn({ error }, 'receive attempt hydration failed'))

    const handler = (event: ClipboardRealtimeEvent) => {
      if (cancelled) return

      if (transferProgressDebugEnabled && event.eventType === 'file-transfer.status_changed') {
        const payload = event.payload as {
          transferId: string
          entryId: string
          status: string
          reason?: string | null
        }
        if (transferProgressDebugEnabled) {
          log.debug(
            {
              transferId: payload.transferId,
              entryId: payload.entryId,
              status: payload.status,
              reason: payload.reason ?? null,
            },
            'file transfer status changed'
          )
        }
      }

      if (transferProgressDebugEnabled && event.eventType === 'file-transfer.progress') {
        const payload = event.payload as {
          transferId: string
          entryId?: string | null
          peerId: string
          direction: 'Sending' | 'Receiving'
          bytesTransferred: number
          totalBytes?: number | null
        }
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
      }

      const reduction = reducer.reduce(event)
      for (const action of reduction.actions) dispatch(action)
    }

    const unsubscribe = daemonWs.subscribe(['file-transfer', 'clipboard'], handler)

    return () => {
      cancelled = true
      unsubscribe()
    }
  }, [dispatch])
}
