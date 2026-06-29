import { useEffect, useState } from 'react'
import { getBlobImageObjectUrl } from '@/api/daemon/blob-image-cache'

/**
 * Resolve an image descriptor into a displayable `<img src>`.
 *
 * - `data:` URLs (inline content) pass through unchanged — no fetch.
 * - Daemon blob paths are fetched with session auth and wrapped in a stable
 *   `blob:` object URL via {@link getBlobImageObjectUrl}, so the rendered `src`
 *   never carries a short-lived session token (which would 401 once expired).
 *
 * @param descriptor `data:` URL, daemon blob path, or `null`.
 * @param enabled When false, resolves to `null` without fetching — lets callers
 *   gate the byte pull behind an explicit action (e.g. D6 large-image reveal).
 * @returns The resolved object URL / data URL, or `null` while pending or gated.
 */
export function useBlobImageObjectUrl(descriptor: string | null, enabled = true): string | null {
  const [url, setUrl] = useState<string | null>(null)

  useEffect(() => {
    if (!descriptor || !enabled) {
      setUrl(null)
      return
    }
    if (descriptor.startsWith('data:')) {
      setUrl(descriptor)
      return
    }

    let cancelled = false
    setUrl(null)
    getBlobImageObjectUrl(descriptor)
      .then(resolved => {
        if (!cancelled) setUrl(resolved)
      })
      .catch(() => {
        if (!cancelled) setUrl(null)
      })
    return () => {
      cancelled = true
    }
  }, [descriptor, enabled])

  return url
}
