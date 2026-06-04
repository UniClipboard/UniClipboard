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
  /** Filenames the daemon already knows (from V3 envelope). May be empty. */
  filenames?: string[]
}

export function useClipboardEventStream({
  enabled = true,
  throttleMs = 300,
  onLocalItem,
  onRemoteInvalidate,
  onDeleted,
}: UseClipboardEventStreamOptions): void {
  const dispatch = useAppDispatch()
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

    // throttle 用的 trailing-edge timeout —— 用 effect-local 变量持有，
    // cleanup 直接闭包捕获最新值；这样 react-doctor 不会再误报
    // "timeoutRef.current 会在 cleanup 跑时变化"，因为根本不是 ref。
    // 后果上等价：每次 effect 重订阅都会建一个新 timeoutId 槽位，旧的
    // 在前一个 cleanup 里已经清掉。
    let pendingTimeoutId: number | null = null

    // 本地 new_content 合并:快速连续复制时,每条事件原本各拉一次完整列表
    // (getClipboardEntries(50,0)) —— 那是 daemon 限流的主要放大源。这里把窗口
    // 内的多次拉取压成一次:窗口内累积的 entryId 进 set,trailing 一拍用同一次
    // 列表请求一并取回(前 50 条已覆盖这段时间全部新增 entry),再逐个回调
    // onLocalItem,不丢任何一条。
    let localFlushTimer: number | null = null
    let lastLocalFetch: number | undefined = undefined
    const pendingLocalIds = new Set<string>()

    const flushLocalFetch = () => {
      lastLocalFetch = Date.now()
      const ids = Array.from(pendingLocalIds)
      pendingLocalIds.clear()
      void getClipboardEntries(50, 0)
        .then(response => {
          log.info(
            { status: response.status, requested: ids.length },
            'local list reload (coalesced)'
          )
          if (response.status !== 'ready' || !response.entries) return
          const entries = response.entries
          for (const id of ids) {
            const entry = entries.find(e => e.id === id)
            if (!entry) {
              log.warn({ entryId: id }, 'local entry not found in list reload')
              continue
            }
            onLocalItemRef.current(transformDaemonDtoToItemResponse(entry))
          }
        })
        .catch(err => log.error({ err }, 'Failed to fetch local clipboard entries'))
    }

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
            filenameCount: payload.filenames?.length ?? 0,
          },
          'clipboard.incoming_pending payload'
        )
        const pending: PendingClipboardEntry = {
          entryId: payload.entryId,
          fromDevice: payload.fromDevice,
          totalBytes: payload.totalBytes ?? null,
          filenames: payload.filenames ?? [],
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
          // leading + trailing 合并:首条立即拉取(无可感延迟),窗口内后续 entryId
          // 累积到 set,在 trailing 一拍用一次列表请求一并取回。
          pendingLocalIds.add(payload.entryId)
          const now = Date.now()
          const sinceLast =
            lastLocalFetch === undefined ? Number.POSITIVE_INFINITY : now - lastLocalFetch
          if (sinceLast >= throttleMs) {
            if (localFlushTimer !== null) {
              clearTimeout(localFlushTimer)
              localFlushTimer = null
            }
            flushLocalFetch()
          } else if (localFlushTimer === null) {
            localFlushTimer = window.setTimeout(() => {
              localFlushTimer = null
              flushLocalFetch()
            }, throttleMs - sinceLast)
          }
          return
        }

        // Remote: throttled full list reload.
        const now = Date.now()
        const lastReload = lastReloadTimestampRef.current
        if (lastReload === undefined || now - lastReload >= throttleMs) {
          lastReloadTimestampRef.current = now
          if (pendingTimeoutId !== null) {
            clearTimeout(pendingTimeoutId)
            pendingTimeoutId = null
          }
          onRemoteInvalidateRef.current()
          return
        }

        if (pendingTimeoutId === null) {
          const delay = throttleMs - (now - lastReload)
          pendingTimeoutId = window.setTimeout(() => {
            lastReloadTimestampRef.current = Date.now()
            onRemoteInvalidateRef.current()
            pendingTimeoutId = null
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
      if (pendingTimeoutId !== null) {
        clearTimeout(pendingTimeoutId)
        pendingTimeoutId = null
      }
      if (localFlushTimer !== null) {
        clearTimeout(localFlushTimer)
        localFlushTimer = null
      }
      unsubscribe()
    }
  }, [enabled, throttleMs, dispatch])
}
