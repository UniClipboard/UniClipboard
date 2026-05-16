import { useCallback, useEffect, useState } from 'react'
import type { ClipboardItemResponse } from '@/api/clipboardItems'
import { getClipboardEntries } from '@/api/daemon/clipboard'
import { transformDaemonDtoToItemResponse } from '@/lib/clipboard-transform'
import { createLogger } from '@/lib/logger'
import { useClipboardEventStream } from './useClipboardEventStream'
import { useEncryptionSessionState } from './useEncryptionSessionState'

const log = createLogger('use-clipboard-collection')

const PAGE_SIZE = 50

export interface ClipboardCollectionResult {
  items: ClipboardItemResponse[]
  loading: boolean
  isLocked: boolean
  encryptionReady: boolean
  reload: () => Promise<void>
}

export function useClipboardCollection(): ClipboardCollectionResult {
  const { encryptionReady, isLocked } = useEncryptionSessionState()
  const [items, setItems] = useState<ClipboardItemResponse[]>([])
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
      const transformedItems = result.entries?.map(transformDaemonDtoToItemResponse) ?? []
      setItems(transformedItems)
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
