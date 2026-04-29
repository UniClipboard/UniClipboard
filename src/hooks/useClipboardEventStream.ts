import { useEffect, useRef } from 'react'
import type { ClipboardItemResponse } from '@/api/clipboardItems'
import { getClipboardEntries } from '@/api/daemon/clipboard'
import { transformDaemonDtoToItemResponse } from '@/lib/clipboard-transform'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import {
  addPendingEntry,
  removePendingEntry,
  type PendingClipboardEntry,
} from '@/store/slices/clipboardSlice'
import { setEntryTransferStatus } from '@/store/slices/fileTransferSlice'

const log = createLogger('use-clipboard-event-stream')

export interface UseClipboardEventStreamOptions {
  enabled?: boolean
  throttleMs?: number
  onLocalItem: (item: ClipboardItemResponse) => void
  onRemoteInvalidate: () => void
  onDeleted: (id: string) => void
}

/**
 * Payload for `clipboard.new_content` daemon WS events.
 * (Matches the Rust `ClipboardNewContentEvent` serde shape.)
 */
interface ClipboardNewContentPayload {
  entryId: string
  preview: string
  origin: string // "local" | "remote"
}

/**
 * Payload for `clipboard.incoming_pending` daemon WS events.
 *
 * Daemon emits this right after V3 envelope decode (before blob fetch),
 * carrying the receiver-side entry_id that the eventual `new_content`
 * event will reuse. The frontend uses this to render a placeholder card
 * with a live transfer progress bar before the real entry persists.
 */
interface ClipboardIncomingPendingPayload {
  entryId: string
  fromDevice: string
  totalBytes?: number | null
}

export function useClipboardEventStream({
  enabled = true,
  throttleMs = 300,
  onLocalItem,
  onRemoteInvalidate,
  onDeleted,
}: UseClipboardEventStreamOptions): void {
  const dispatch = useAppDispatch()
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
    if (!enabled) {
      log.warn('disabled (enabled=false, likely encryptionReady=false)')
      return
    }
    log.info('subscribing to clipboard topic')

    const handler = (event: { topic: string; eventType: string; payload: unknown }) => {
      log.info({ eventType: event.eventType }, 'received event')
      // 接收端 inbound 流程一开始就发的"占位事件":让列表立刻出现一行
      // 灰色 placeholder + 进度条,而不是要等 fetch + capture 全流程结束。
      if (event.eventType === 'clipboard.incoming_pending') {
        const payload = event.payload as ClipboardIncomingPendingPayload
        log.info(
          {
            entryId: payload.entryId,
            fromDevice: payload.fromDevice,
            totalBytes: payload.totalBytes ?? null,
          },
          'clipboard.incoming_pending payload'
        )
        const pending: PendingClipboardEntry = {
          entryId: payload.entryId,
          fromDevice: payload.fromDevice,
          totalBytes: payload.totalBytes ?? null,
          createdAt: Date.now(),
        }
        dispatch(addPendingEntry(pending))
        // 让 ItemRow 立即走 transferring 视觉(spinner + ring),
        // 后续 file-transfer.progress 事件会接着填充字节进度。
        dispatch(
          setEntryTransferStatus({
            entryId: payload.entryId,
            status: 'transferring',
            reason: null,
          })
        )
        return
      }

      // Route clipboard.new_content to onLocalItem / onRemoteInvalidate.
      if (event.eventType === 'clipboard.new_content') {
        const payload = event.payload as ClipboardNewContentPayload
        log.info(
          { entryId: payload.entryId, origin: payload.origin },
          'clipboard.new_content payload'
        )
        // 真实 entry 已经持久化了,占位卡片可以下线 ——
        // 列表 refresh 拿到的真实 ClipboardItemResponse 会接替它。
        dispatch(removePendingEntry(payload.entryId))
        if (payload.origin === 'local') {
          // Fetch single entry from daemon list endpoint (matching clipboardSlice pattern)
          void getClipboardEntries(50, 0)
            .then(response => {
              log.info(
                { status: response.status, entriesCount: response.entries?.length ?? 0 },
                'getClipboardEntries response'
              )
              if (response.status !== 'ready' || !response.entries) return null
              const entry = response.entries.find(e => e.id === payload.entryId)
              log.info({ entryId: payload.entryId, found: !!entry }, 'found entry for id')
              if (!entry) return null
              return transformDaemonDtoToItemResponse(entry)
            })
            .then(item => {
              log.info({ hasItem: !!item }, 'onLocalItem called with item')
              if (item) onLocalItemRef.current(item)
            })
            .catch(err => log.error({ err }, 'Failed to fetch local clipboard entry'))
          return
        }

        // Remote: throttled full list reload.
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
        return
      }

      // Note: clipboard.deleted is never emitted by the daemon.
      // The onDeleted callback is retained for API symmetry but will never fire.
      void onDeletedRef
    }

    const unsubscribe = daemonWs.subscribe(['clipboard'], handler)

    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
        timeoutRef.current = null
      }
      unsubscribe()
    }
  }, [enabled, throttleMs])
}
