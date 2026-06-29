import { useEffect, useState } from 'react'
import { getResourceImageUrl } from '@/api/clipboardItems'
import { getClipboardEntryResource } from '@/api/daemon/clipboard'
import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'

/**
 * Cache the (token-free) image descriptor per entry so scrolling a virtualized
 * list back to a row doesn't re-hit the daemon for its resource metadata. The
 * descriptor is a `data:` URL or a daemon blob path — cheap strings, no bytes.
 * The actual image bytes are cached separately (and LRU-bounded) by
 * {@link useBlobImageObjectUrl}.
 */
const descriptorCache = new Map<string, string | null>()

export function useResourceImageUrl(entryId: string): string | null {
  const [descriptor, setDescriptor] = useState<string | null>(
    () => descriptorCache.get(entryId) ?? null
  )

  useEffect(() => {
    const cached = descriptorCache.get(entryId)
    if (cached !== undefined) {
      setDescriptor(cached)
      return
    }
    // Row reuse (virtualized list) can hand this hook a new entry id; drop the
    // previous descriptor immediately so a stale thumbnail never lingers while
    // the new resource lookup is in flight.
    setDescriptor(null)
    let cancelled = false
    getClipboardEntryResource(entryId)
      .then(resource => {
        if (cancelled) return
        const next = resource ? getResourceImageUrl(resource) : null
        descriptorCache.set(entryId, next)
        setDescriptor(next)
      })
      .catch(() => {
        // Don't cache failures, so a remount can retry.
        if (!cancelled) setDescriptor(null)
      })
    return () => {
      cancelled = true
    }
  }, [entryId])

  return useBlobImageObjectUrl(descriptor)
}
