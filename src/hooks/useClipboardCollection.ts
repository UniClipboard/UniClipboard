import { useCallback, useEffect, useState } from 'react'
import { getClipboardEntries } from '@/api/daemon/clipboard'
import type { ClipboardEntry } from '@/lib/clipboard-entry'
import { projectClipboardEntry } from '@/lib/clipboard-transform'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { useClipboardEventStream } from './useClipboardEventStream'
import { useEncryptionSessionState } from './useEncryptionSessionState'

const log = createLogger('use-clipboard-collection')

const PAGE_SIZE = 50

export interface ClipboardCollectionResult {
  items: ClipboardEntry[]
  loading: boolean
  isLocked: boolean
  encryptionReady: boolean
  reload: () => Promise<void>
}

export function useClipboardCollection(): ClipboardCollectionResult {
  const { encryptionReady, isLocked } = useEncryptionSessionState()
  const [items, setItems] = useState<ClipboardEntry[]>([])
  const [loading, setLoading] = useState(true)

  const reload = useCallback(async () => {
    if (!encryptionReady) {
      setItems([])
      setLoading(!isLocked)
      return
    }

    setLoading(true)
    try {
      const result = await getClipboardEntries(PAGE_SIZE, 0)
      if (result.status === 'not_ready') {
        setItems([])
        return
      }
      setItems(result.entries?.map(projectClipboardEntry) ?? [])
    } catch (err) {
      log.error({ err }, 'Failed to load clipboard items')
    } finally {
      setLoading(false)
    }
  }, [encryptionReady, isLocked])

  useEffect(() => {
    if (!encryptionReady) {
      setItems([])
      setLoading(!isLocked)
      return
    }

    void reload()
  }, [encryptionReady, isLocked, reload])

  // D8 resync: after the WS reconnects (e.g. daemon restart), the live event
  // stream resumes but events missed during the outage are not replayed.
  // Re-pull the full list — getClipboardEntries is idempotent, so a fresh
  // snapshot reconciles any entries that arrived while we were disconnected.
  useEffect(() => {
    const unsubscribe = daemonWs.onReconnect(() => {
      if (encryptionReady) {
        void reload()
      }
    })
    return unsubscribe
  }, [encryptionReady, reload])

  useClipboardEventStream({
    enabled: encryptionReady,
    onLocalItem: item => {
      setItems(prev => (prev.some(existing => existing.id === item.id) ? prev : [item, ...prev]))
    },
    onRemoteInvalidate: () => {
      void reload()
    },
    onDeleted: id => {
      setItems(prev => prev.filter(item => item.id !== id))
    },
  })

  return { items, loading, isLocked, encryptionReady, reload }
}
