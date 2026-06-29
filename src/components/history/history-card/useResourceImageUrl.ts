import { useEffect, useState } from 'react'
import { resolveResourceImageUrl } from '@/api/clipboardItems'
import { getClipboardEntryResource } from '@/api/daemon/clipboard'

const imageUrlCache = new Map<string, string | null>()

export function useResourceImageUrl(entryId: string): string | null {
  const [imageUrl, setImageUrl] = useState<string | null>(() => imageUrlCache.get(entryId) ?? null)

  useEffect(() => {
    if (imageUrlCache.has(entryId)) {
      setImageUrl(imageUrlCache.get(entryId) ?? null)
      return
    }
    let cancelled = false
    getClipboardEntryResource(entryId)
      .then(resource => {
        if (cancelled) return
        const url = resource ? resolveResourceImageUrl(resource) : null
        imageUrlCache.set(entryId, url)
        setImageUrl(url)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [entryId])

  return imageUrl
}
