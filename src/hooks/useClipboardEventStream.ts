import { useEffect, useRef } from 'react'
import { getClipboardEntries } from '@/api/daemon/clipboard'
import type { ClipboardEntry } from '@/lib/clipboard-entry'
import { projectClipboardEntry } from '@/lib/clipboard-transform'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import { createClipboardEventReducer } from './clipboardEventReducer'
import type { ClipboardEventReducerEffect, ClipboardRealtimeEvent } from './clipboardEventReducer'

const log = createLogger('use-clipboard-event-stream')

export interface UseClipboardEventStreamOptions {
  enabled?: boolean
  throttleMs?: number
  onLocalItem: (item: ClipboardEntry) => void
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
  const dispatch = useAppDispatch()
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

    let remoteInvalidateTimer: number | null = null
    let localFlushTimer: number | null = null
    let cancelled = false
    const reducer = createClipboardEventReducer({ now: () => Date.now(), throttleMs })

    const flushLocalFetch = (ids: string[]) => {
      void getClipboardEntries(50, 0)
        .then(response => {
          if (cancelled) return
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
            onLocalItemRef.current(projectClipboardEntry(entry))
          }
        })
        .catch(err => {
          if (!cancelled) log.error({ err }, 'Failed to fetch local clipboard entries')
        })
    }

    const applyEffects = (effects: ClipboardEventReducerEffect[]) => {
      for (const effect of effects) {
        if (effect.type === 'flushLocalEntries') {
          if (localFlushTimer !== null) {
            clearTimeout(localFlushTimer)
            localFlushTimer = null
          }
          flushLocalFetch(effect.entryIds)
          continue
        }

        if (effect.type === 'scheduleLocalFlush') {
          if (localFlushTimer !== null) continue
          localFlushTimer = window.setTimeout(() => {
            localFlushTimer = null
            applyEffects(reducer.flushLocal().effects)
          }, effect.delayMs)
          continue
        }

        if (effect.type === 'invalidateRemote') {
          if (remoteInvalidateTimer !== null) {
            clearTimeout(remoteInvalidateTimer)
            remoteInvalidateTimer = null
          }
          onRemoteInvalidateRef.current()
          continue
        }

        if (effect.type === 'scheduleRemoteInvalidate') {
          if (remoteInvalidateTimer !== null) continue
          remoteInvalidateTimer = window.setTimeout(() => {
            remoteInvalidateTimer = null
            applyEffects(reducer.flushRemote().effects)
          }, effect.delayMs)
        }
      }
    }

    const handler = (event: ClipboardRealtimeEvent) => {
      log.info({ eventType: event.eventType }, 'received event')
      const reduction = reducer.reduce(event)
      for (const action of reduction.actions) dispatch(action)
      applyEffects(reduction.effects)

      // Note: clipboard.deleted is never emitted by the daemon.
      // The onDeleted callback is retained for API symmetry but will never fire.
      void onDeletedRef
    }

    const unsubscribe = daemonWs.subscribe(['clipboard'], handler)

    return () => {
      cancelled = true
      if (remoteInvalidateTimer !== null) {
        clearTimeout(remoteInvalidateTimer)
        remoteInvalidateTimer = null
      }
      if (localFlushTimer !== null) {
        clearTimeout(localFlushTimer)
        localFlushTimer = null
      }
      unsubscribe()
    }
  }, [enabled, throttleMs, dispatch])
}
