import { useEffect } from 'react'
import { listOutboundSnapshots } from '@/api/outbound_progress'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import { replaceOutboundSnapshots } from '@/store/slices/outboundProgressSlice'

const log = createLogger('use-outbound-progress')

/** Coalesce bursts of invalidation hints into one snapshot read. */
const REFRESH_DEBOUNCE_MS = 150

/**
 * Keeps the sender-side outbound progress cache equal to the Engine's snapshot.
 *
 * `file-transfer.outbound_progress_changed` carries no state; it only
 * invalidates. The full list is re-read on mount, on that hint, after a
 * WebSocket reconnect (events missed while down are not replayed) and when the
 * window returns to the foreground. Reads are sequenced so a slow older
 * response can never overwrite a newer one.
 *
 * Call once in a top-level component, next to `useTransferProgress`.
 */
export function useOutboundProgress(): void {
  const dispatch = useAppDispatch()

  useEffect(() => {
    let cancelled = false
    let issued = 0
    let applied = 0
    let timer: ReturnType<typeof setTimeout> | null = null

    const refresh = () => {
      const ticket = ++issued
      void listOutboundSnapshots()
        .then(snapshots => {
          if (cancelled || ticket < applied) return
          applied = ticket
          dispatch(replaceOutboundSnapshots(snapshots))
        })
        .catch(error => log.warn({ error }, 'outbound progress refresh failed'))
    }
    const scheduleRefresh = () => {
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        timer = null
        refresh()
      }, REFRESH_DEBOUNCE_MS)
    }
    const onVisibility = () => {
      if (document.visibilityState === 'visible') scheduleRefresh()
    }

    refresh()
    const unsubscribe = daemonWs.subscribe(['file-transfer'], event => {
      if (event.eventType === 'file-transfer.outbound_progress_changed') scheduleRefresh()
    })
    const unsubscribeReconnect = daemonWs.onReconnect(scheduleRefresh)
    document.addEventListener('visibilitychange', onVisibility)

    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
      unsubscribe()
      unsubscribeReconnect()
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [dispatch])
}
