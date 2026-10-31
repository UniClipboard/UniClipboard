import { useEffect, useRef } from 'react'
import {
  getBlobImageObjectUrl,
  peekBlobImageObjectUrl,
  releaseBlobImage,
  retainBlobImage,
} from '@/api/daemon/blob-image-cache'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { imageRetryDelay } from '@/lib/image-retry'

/** Preload only the bounded history window, retaining shared URLs until its records leave. */
export function useHistoryWindowImages(items: DisplayClipboardItem[]): void {
  const retained = useRef(new Set<string>())
  useEffect(() => {
    const paths = new Set(
      items
        .filter(item => item.type === 'image')
        .map(item => `/clipboard/entries/${encodeURIComponent(item.id)}/thumbnail`)
    )
    for (const path of retained.current) if (!paths.has(path)) releaseBlobImage(path, 'buffer')
    for (const path of paths) if (!retained.current.has(path)) retainBlobImage(path, 'buffer')
    retained.current = paths
    let cancelled = false
    let cancelDelay: (() => void) | undefined
    const preload = async () => {
      let queue = [...paths]
      let attempt = 0
      while (queue.length && !cancelled) {
        const busy: string[] = []
        for (const path of queue) {
          if (cancelled) return
          if (!retained.current.has(path) || peekBlobImageObjectUrl(path)) continue
          try {
            await getBlobImageObjectUrl(path)
          } catch (error) {
            if (
              error instanceof DaemonApiError &&
              error.code === DaemonErrorCode.SERVICE_UNAVAILABLE
            )
              busy.push(path)
          }
        }
        queue = busy
        if (queue.length && !cancelled) {
          await new Promise<void>(resolve => {
            const timer = setTimeout(resolve, imageRetryDelay(attempt))
            cancelDelay = () => {
              clearTimeout(timer)
              resolve()
            }
          })
          attempt = Math.min(attempt + 1, 4)
        }
      }
    }
    void preload()
    return () => {
      cancelled = true
      cancelDelay?.()
    }
  }, [items])
  useEffect(
    () => () => {
      for (const path of retained.current) releaseBlobImage(path, 'buffer')
      retained.current.clear()
    },
    []
  )
}
