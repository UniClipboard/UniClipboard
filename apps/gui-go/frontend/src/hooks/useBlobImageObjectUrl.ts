import { useEffect, useState } from 'react'
import {
  getBlobImageObjectUrl,
  peekBlobImageObjectUrl,
  retainBlobImage,
  releaseBlobImage,
} from '@/api/daemon/blob-image-cache'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { imageRetryDelay } from '@/lib/image-retry'

function scheduleRetry(callback: () => void, delay: number): () => void {
  const timer = setTimeout(callback, delay)
  return () => clearTimeout(timer)
}

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
  // Track the descriptor the resolved URL belongs to so a changed descriptor
  // invalidates the old URL during the same render (before the effect runs),
  // instead of painting one stale frame of the previous entry's image.
  const [, setResolved] = useState<{ descriptor: string | null; url: string | null }>({
    descriptor: null,
    url: null,
  })

  const url =
    !descriptor || !enabled
      ? null
      : descriptor.startsWith('data:')
        ? descriptor
        : peekBlobImageObjectUrl(descriptor)

  useEffect(() => {
    if (!descriptor || !enabled || descriptor.startsWith('data:')) return

    retainBlobImage(descriptor)
    let cancelled = false
    let cancelRetry: (() => void) | undefined
    let attempts = 0
    const load = () => {
      getBlobImageObjectUrl(descriptor)
        .then(objectUrl => {
          if (!cancelled) setResolved({ descriptor, url: objectUrl })
        })
        .catch(error => {
          if (cancelled) return
          if (
            error instanceof DaemonApiError &&
            error.code === DaemonErrorCode.SERVICE_UNAVAILABLE
          ) {
            cancelRetry = scheduleRetry(load, imageRetryDelay(attempts))
            attempts = Math.min(attempts + 1, 4)
          } else {
            setResolved({ descriptor, url: null })
          }
        })
    }
    load()
    return () => {
      cancelled = true
      cancelRetry?.()
      releaseBlobImage(descriptor)
    }
  }, [descriptor, enabled])

  return url
}
