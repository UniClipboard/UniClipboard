import { useEffect, useRef, useState } from 'react'
import { clipboardPreviewCache, type ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

export type ClipboardPreviewState = ClipboardPreviewData

export interface ClipboardPreviewResult {
  preview: ClipboardPreviewState | null
  loading: boolean
  error: string | null
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

  return { preview, loading, error }
}
