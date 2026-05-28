import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification'
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppSelector } from '@/store/hooks'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'

const BATCH_WINDOW_MS = 500

/**
 * Hook that watches transfer state changes and emits batched system notifications.
 *
 * - Start notifications are batched: a single notification fires after BATCH_WINDOW_MS
 *   summarising how many files are being synced and to which device.
 * - Completion notifications are batched the same way.
 * - Error notifications fire immediately (no batching).
 *
 * Call once in a top-level component (e.g. ClipboardContent) to activate.
 */
export function useFileSyncNotifications(): void {
  const { t } = useTranslation()
  const activeTransfers = useAppSelector(state => state.fileTransfer.activeTransfers)

  // Track previous transfer statuses to detect transitions
  const prevStatusRef = useRef<Record<string, TransferProgressInfo['status']>>({})
  const permittedRef = useRef<boolean | null>(null)

  // Batching refs
  const startBatchTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const completeBatchTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const pendingStarts = useRef<Map<string, string>>(new Map()) // peerId -> count aggregated later
  const pendingCompletes = useRef<Map<string, string>>(new Map())

  // Check notification permission on mount
  useEffect(() => {
    const checkPermission = async () => {
      try {
        let permitted = await isPermissionGranted()
        if (!permitted) {
          const result = await requestPermission()
          permitted = result === 'granted'
        }
        permittedRef.current = permitted
      } catch {
        permittedRef.current = false
      }
    }
    void checkPermission()
  }, [])

  useEffect(() => {
    if (permittedRef.current === false) return

    const prevStatus = prevStatusRef.current
    const currentIds = new Set(Object.keys(activeTransfers))

    // Detect new transfers (start)
    for (const [id, transfer] of Object.entries(activeTransfers)) {
      if (!(id in prevStatus) && transfer.status === 'active') {
        // New transfer started
        pendingStarts.current.set(id, transfer.peerId)
      }
    }

    // Detect status transitions
    for (const [id, transfer] of Object.entries(activeTransfers)) {
      const prev = prevStatus[id]

      if (prev === 'active' && transfer.status === 'completed') {
        pendingCompletes.current.set(id, transfer.peerId)
      }

      if (prev === 'active' && transfer.status === 'failed') {
        // Error notifications fire immediately
        const errorMsg = transfer.errorMessage || t('clipboard.transfer.failed')
        void notify(
          t('clipboard.notification.syncFailed'),
          t('clipboard.notification.syncFailedDetail', { reason: errorMsg })
        )
      }

      // Also handle transfers that appeared already as 'failed' (unlikely but safe)
      if (prev === undefined && transfer.status === 'failed') {
        const errorMsg = transfer.errorMessage || t('clipboard.transfer.failed')
        void notify(
          t('clipboard.notification.syncFailed'),
          t('clipboard.notification.syncFailedDetail', { reason: errorMsg })
        )
      }
    }

    // Schedule batched start notification
    if (pendingStarts.current.size > 0 && startBatchTimer.current === null) {
      startBatchTimer.current = setTimeout(() => {
        const starts = pendingStarts.current
        if (starts.size > 0) {
          // Group by peerId
          const byPeer = new Map<string, number>()
          for (const peerId of starts.values()) {
            byPeer.set(peerId, (byPeer.get(peerId) ?? 0) + 1)
          }
          for (const [peerId, count] of byPeer) {
            const device = peerId.slice(0, 8)
            const body =
              count === 1
                ? t('clipboard.notification.syncStartSingle', { device })
                : t('clipboard.notification.syncStartBatch', { count, device })
            void notify('UniClipboard', body)
          }
        }
        pendingStarts.current = new Map()
        startBatchTimer.current = null
      }, BATCH_WINDOW_MS)
    }

    // Schedule batched completion notification
    if (pendingCompletes.current.size > 0 && completeBatchTimer.current === null) {
      completeBatchTimer.current = setTimeout(() => {
        const completes = pendingCompletes.current
        if (completes.size > 0) {
          const count = completes.size
          const body =
            count === 1
              ? t('clipboard.notification.syncCompleteSingle')
              : t('clipboard.notification.syncCompleteBatch', { count })
          void notify('UniClipboard', body)
        }
        pendingCompletes.current = new Map()
        completeBatchTimer.current = null
      }, BATCH_WINDOW_MS)
    }

    // Update previous status snapshot
    const newStatus: Record<string, TransferProgressInfo['status']> = {}
    for (const [id, transfer] of Object.entries(activeTransfers)) {
      newStatus[id] = transfer.status
    }
    // Keep status for IDs that were removed (so we don't re-trigger)
    // but only for one cycle
    prevStatusRef.current = newStatus

    // Cleanup removed IDs from prev (they are gone)
    for (const id of Object.keys(prevStatus)) {
      if (!currentIds.has(id)) {
        delete prevStatusRef.current[id]
      }
    }

    return () => {
      if (startBatchTimer.current) {
        clearTimeout(startBatchTimer.current)
        startBatchTimer.current = null
      }
      if (completeBatchTimer.current) {
        clearTimeout(completeBatchTimer.current)
        completeBatchTimer.current = null
      }
    }
  }, [activeTransfers, t])
}

async function notify(title: string, body: string): Promise<void> {
  try {
    const permitted = await isPermissionGranted()
    if (permitted) {
      sendNotification({ title, body })
    }
  } catch {
    // Silently ignore notification errors in non-Tauri environments
  }
}
