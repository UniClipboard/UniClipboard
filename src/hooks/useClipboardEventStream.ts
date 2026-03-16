import { listen } from '@tauri-apps/api/event'
import { useEffect, useRef } from 'react'
import { getClipboardEntry } from '@/api/clipboardItems'
import type { ClipboardItemResponse } from '@/api/clipboardItems'
import type { ClipboardEvent } from '@/types/events'

export interface UseClipboardEventStreamOptions {
  enabled?: boolean
  throttleMs?: number
  onLocalItem: (item: ClipboardItemResponse) => void
  onRemoteInvalidate: () => void
  onDeleted: (id: string) => void
}

export function useClipboardEventStream({
  enabled = true,
  throttleMs = 300,
  onLocalItem,
  onRemoteInvalidate,
  onDeleted,
}: UseClipboardEventStreamOptions): void {
  const timeoutRef = useRef<number | null>(null)
  const lastReloadTimestampRef = useRef<number | undefined>(undefined)
  const onLocalItemRef = useRef(onLocalItem)
  const onRemoteInvalidateRef = useRef(onRemoteInvalidate)
  const onDeletedRef = useRef(onDeleted)

  useEffect(() => {
    onLocalItemRef.current = onLocalItem
    onRemoteInvalidateRef.current = onRemoteInvalidate
    onDeletedRef.current = onDeleted
  }, [onDeleted, onLocalItem, onRemoteInvalidate])

  useEffect(() => {
    if (!enabled) return

    let cancelled = false

    const unlistenPromise = listen<ClipboardEvent>('clipboard://event', event => {
      if (cancelled) return

      if (event.payload.type === 'Deleted' && event.payload.entry_id) {
        onDeletedRef.current(event.payload.entry_id)
        return
      }

      if (event.payload.type !== 'NewContent' || !event.payload.entry_id) return

      if (event.payload.origin === 'local') {
        void getClipboardEntry(event.payload.entry_id).then(item => {
          if (cancelled || !item) return
          onLocalItemRef.current(item)
        })
        return
      }

      const now = Date.now()
      const lastReload = lastReloadTimestampRef.current

      if (lastReload === undefined || now - lastReload >= throttleMs) {
        lastReloadTimestampRef.current = now
        if (timeoutRef.current) {
          clearTimeout(timeoutRef.current)
          timeoutRef.current = null
        }
        onRemoteInvalidateRef.current()
        return
      }

      if (!timeoutRef.current) {
        const delay = throttleMs - (now - lastReload)
        timeoutRef.current = window.setTimeout(() => {
          lastReloadTimestampRef.current = Date.now()
          onRemoteInvalidateRef.current()
          timeoutRef.current = null
        }, delay)
      }
    })

    return () => {
      cancelled = true
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
        timeoutRef.current = null
      }
      unlistenPromise.then(fn => fn())
    }
  }, [enabled, throttleMs])
}
