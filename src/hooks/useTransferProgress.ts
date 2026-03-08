import { listen, UnlistenFn } from '@tauri-apps/api/event'
import { useEffect } from 'react'
import { useAppDispatch } from '@/store/hooks'
import {
  updateTransferProgress,
  clearStaleTransfers,
  TransferProgressPayload,
} from '@/store/slices/transferSlice'

/**
 * Hook that subscribes to transfer://progress Tauri events and dispatches
 * progress updates to the Redux store. Also runs periodic cleanup of
 * completed transfers.
 *
 * Call once from a top-level component (e.g., DashboardPage).
 */
export function useTransferProgress(): void {
  const dispatch = useAppDispatch()

  useEffect(() => {
    let unlisten: UnlistenFn | undefined

    listen<TransferProgressPayload>('transfer://progress', event => {
      dispatch(updateTransferProgress(event.payload))
    }).then(fn => {
      unlisten = fn
    })

    // Periodically clear completed/stale transfers
    const interval = setInterval(() => {
      dispatch(clearStaleTransfers())
    }, 2000)

    return () => {
      unlisten?.()
      clearInterval(interval)
    }
  }, [dispatch])
}
