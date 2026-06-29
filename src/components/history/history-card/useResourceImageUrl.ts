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
    // Row reuse (virtualized list) can hand this hook a new entry id; drop the
    // previous entry's image immediately so a stale thumbnail never lingers
    // while the new fetch is in flight.
    setImageUrl(null)
    let cancelled = false
    getClipboardEntryResource(entryId)
      .then(resource => {
        if (cancelled) return
        const url = resource ? resolveResourceImageUrl(resource) : null
        imageUrlCache.set(entryId, url)
        setImageUrl(url)
      })
      .catch(() => {
        // Clear on failure too — an unhandled rejection must not leave the prior
        // row's image stuck on this entry. Not cached, so a remount can retry.
        if (!cancelled) setImageUrl(null)
      })
    return () => {
      cancelled = true
    }
  }, [entryId])

  return imageUrl
}
