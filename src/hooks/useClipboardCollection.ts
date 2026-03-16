import { useCallback, useEffect, useState } from 'react'
import { useClipboardEventStream } from './useClipboardEventStream'
import { useEncryptionSessionState } from './useEncryptionSessionState'
import { getClipboardItems, OrderBy } from '@/api/clipboardItems'
import type { ClipboardItemResponse } from '@/api/clipboardItems'

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
      setLoading(false)
      return
    }

    setLoading(true)
    try {
      const result = await getClipboardItems(OrderBy.ActiveTimeDesc, PAGE_SIZE, 0)
      if (result.status === 'not_ready') {
        setItems([])
        return
      }
      setItems(result.items)
    } catch (err) {
      console.error('Failed to load clipboard items:', err)
    } finally {
      setLoading(false)
    }
  }, [encryptionReady])

  useEffect(() => {
    if (!encryptionReady) {
      setItems([])
      setLoading(false)
      return
    }

    void reload()
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
