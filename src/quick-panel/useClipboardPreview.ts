import { useEffect, useRef, useState } from 'react'
import { isImageType, resolveResourceImageUrl } from '@/api/clipboardItems'
import { getClipboardEntryDetail, getClipboardEntryResource } from '@/api/daemon/clipboard'
import { parseFileNamesFromUriList } from '@/lib/clipboard-utils'

export interface ClipboardPreviewState {
  entryId: string
  contentType: 'text' | 'image' | 'file'
  sizeBytes: number
  textContent?: string
  imageUrl?: string
  fileNames?: string[]
}

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
        const resource = await getClipboardEntryResource(entryId)

        if (currentRequestId !== requestIdRef.current) return

        if (!resource) {
          setError('Preview unavailable')
          return
        }

        if (isImageType(resource.mimeType)) {
          const imageUrl = resolveResourceImageUrl(resource)

          setPreview({
            entryId,
            contentType: 'image',
            sizeBytes: resource.sizeBytes,
            imageUrl: imageUrl ?? undefined,
          })
          return
        }

        if (resource.mimeType.includes('uri-list')) {
          const detail = await getClipboardEntryDetail(entryId)

          if (currentRequestId !== requestIdRef.current) return

          if (!detail) {
            setError('Preview unavailable')
            return
          }

          setPreview({
            entryId,
            contentType: 'file',
            sizeBytes: resource.sizeBytes,
            fileNames: parseFileNamesFromUriList(detail.content),
          })
          return
        }

        const detail = await getClipboardEntryDetail(entryId)

        if (currentRequestId !== requestIdRef.current) return

        if (!detail) {
          setError('Preview unavailable')
          return
        }

        setPreview({
          entryId,
          contentType: 'text',
          sizeBytes: detail.sizeBytes,
          textContent: detail.content,
        })
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
