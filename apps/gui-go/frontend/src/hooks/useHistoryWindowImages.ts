import { useEffect, useRef } from 'react'
import {
  getBlobImageObjectUrl,
  peekBlobImageObjectUrl,
  releaseBlobImage,
  retainBlobImage,
} from '@/api/daemon/blob-image-cache'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

/** Preload only the bounded history window, retaining shared URLs until its records leave. */
export function useHistoryWindowImages(items: DisplayClipboardItem[]): void {
  const retained = useRef(new Set<string>())
  useEffect(() => {
    const paths = new Set(
      items
        .filter(item => item.type === 'image')
        .map(item => `/clipboard/entries/${encodeURIComponent(item.id)}/thumbnail`)
    )
    for (const path of retained.current) if (!paths.has(path)) releaseBlobImage(path)
    for (const path of paths) if (!retained.current.has(path)) retainBlobImage(path)
    retained.current = paths
    let cancelled = false
    const preload = async () => {
      for (const path of paths) {
        while (!cancelled && retained.current.has(path) && !peekBlobImageObjectUrl(path)) {
          try {
            await getBlobImageObjectUrl(path)
            break
          } catch (error) {
            if (
              !(error instanceof DaemonApiError) ||
              error.code !== DaemonErrorCode.SERVICE_UNAVAILABLE
            )
              break
            await new Promise(resolve => setTimeout(resolve, 500))
          }
        }
        if (cancelled) return
      }
    }
    void preload()
    return () => {
      cancelled = true
    }
  }, [items])
  useEffect(
    () => () => {
      for (const path of retained.current) releaseBlobImage(path)
      retained.current.clear()
    },
    []
  )
}
