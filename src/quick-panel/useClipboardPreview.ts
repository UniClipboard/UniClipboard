import { useEffect, useRef, useState } from 'react'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import { clipboardPreviewCache, type ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

export type ClipboardPreviewState = ClipboardPreviewData

export interface ClipboardPreviewResult {
  preview: ClipboardPreviewState | null
  loading: boolean
  error: string | null
  /** entry delivery 视图 (来源 + 每对端同步状态);未就绪 / fetch 失败时为 null。 */
  delivery: EntryDeliveryView | null
  /** delivery 单独的 loading 标记,不与 preview loading 合并,二者并发独立。 */
  deliveryLoading: boolean
}

export function useClipboardPreview(entryId: string | null): ClipboardPreviewResult {
  const [preview, setPreview] = useState<ClipboardPreviewState | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const requestIdRef = useRef(0)

  useEffect(() => {
    if (!entryId) {
      requestIdRef.current++
      setPreview(null)
      setLoading(false)
      setError(null)
      return
    }

    const currentRequestId = ++requestIdRef.current
    setLoading(true)
    setError(null)
    setPreview(null)

    void (async () => {
      try {
        const nextPreview = await clipboardPreviewCache.get(entryId)

        if (currentRequestId !== requestIdRef.current) return

        if (!nextPreview) {
          setError('Preview unavailable')
          return
        }

        setPreview(nextPreview)
      } catch (err) {
        if (currentRequestId !== requestIdRef.current) return
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        if (currentRequestId === requestIdRef.current) {
          setLoading(false)
        }
      }
    })()
  }, [entryId])

  const { delivery, loading: deliveryLoading } = useEntryDelivery(entryId)

  return { preview, loading, error, delivery, deliveryLoading }
}
