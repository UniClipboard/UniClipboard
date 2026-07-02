import { useEffect, useState } from 'react'
import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'
import { useResourceImageDescriptor } from '@/hooks/useResourceImageDescriptor'

/**
 * Quick-panel aspect-ratio cache, module-scoped so scrolling the launcher back
 * to a row (or briefly closing and reopening the panel — this file lives
 * alongside the panel module, so the cache survives HistoryPane remounts)
 * does NOT re-flicker aspect ratios in the image wall. Keyed by entry id:
 *
 * - `aspectRatioCache` — the intrinsic aspect ratio measured on `<img.onLoad>`
 *   the first time we saw this entry. The daemon does carry image dimensions
 *   on the entry (`imageWidth`/`imageHeight`) and on the search DTO, but the
 *   quick panel's `DisplayItem` projection currently drops them; caching the
 *   measured value here is enough to keep the masonry layout stable on
 *   subsequent renders without threading dimensions through the projection.
 *
 * Image descriptor resolution (the daemon fetch + blob decode) is shared with
 * the rest of the app via {@link useResourceImageDescriptor} /
 * {@link useBlobImageObjectUrl} — not duplicated here.
 */
const aspectRatioCache = new Map<string, number>()

const aspectRatioListeners = new Set<() => void>()

function notifyAspectRatioChange(): void {
  aspectRatioListeners.forEach(listener => listener())
}

export interface QuickPanelImage {
  /** Resolved `<img src>` (data URL or blob-backed object URL) or null while pending. */
  url: string | null
  /** Cached intrinsic aspect ratio (width / height), or undefined until first onLoad. */
  aspectRatio: number | undefined
}

export interface UseQuickPanelImageOptions {
  /**
   * When false, skip this hook's own descriptor/blob resolution (no daemon
   * call, `url` stays `null`). Used when a parent has already resolved the
   * same entry and is passing the url down instead (see `ImageGridItem`), or
   * when a tile isn't visible yet and shouldn't eagerly fetch (see
   * `PanelItem`). Aspect-ratio tracking stays active regardless — it's a
   * cache read/subscribe, not a fetch.
   */
  enabled?: boolean
}

function initialAspectRatioState(entryId: string): {
  entryId: string
  aspectRatio: number | undefined
} {
  return { entryId, aspectRatio: aspectRatioCache.get(entryId) }
}

export function useQuickPanelImage(
  entryId: string,
  options: UseQuickPanelImageOptions = {}
): QuickPanelImage {
  const { enabled = true } = options
  const descriptor = useResourceImageDescriptor(entryId, enabled)

  const [aspectRatioState, setAspectRatioState] = useState(() => initialAspectRatioState(entryId))

  // Row reuse (filter change) can hand this hook a new entry id without
  // remounting the component. Reset synchronously during render so the first
  // frame for the new id never shows the previous entry's cached ratio.
  if (aspectRatioState.entryId !== entryId) {
    setAspectRatioState(initialAspectRatioState(entryId))
  }

  // Aspect-ratio subscription: pull the cached value on mount (in case another
  // instance measured it first) and re-render whenever any writer publishes a
  // new value for this entry.
  useEffect(() => {
    setAspectRatioState(prev =>
      prev.entryId === entryId ? { ...prev, aspectRatio: aspectRatioCache.get(entryId) } : prev
    )
    const listener = () =>
      setAspectRatioState(prev =>
        prev.entryId === entryId ? { ...prev, aspectRatio: aspectRatioCache.get(entryId) } : prev
      )
    aspectRatioListeners.add(listener)
    return () => {
      aspectRatioListeners.delete(listener)
    }
  }, [entryId])

  const url = useBlobImageObjectUrl(descriptor, enabled)
  return { url, aspectRatio: aspectRatioState.aspectRatio }
}

/**
 * Record the intrinsic aspect ratio for an entry once its `<img>` has loaded.
 * No-op if the value is unchanged, so callers can invoke it unconditionally on
 * every `onLoad`.
 */
export function reportQuickPanelImageAspectRatio(entryId: string, aspectRatio: number): void {
  if (!Number.isFinite(aspectRatio) || aspectRatio <= 0) return
  if (aspectRatioCache.get(entryId) === aspectRatio) return
  aspectRatioCache.set(entryId, aspectRatio)
  notifyAspectRatioChange()
}

/**
 * Read the cached aspect ratio for an entry without subscribing. Used by the
 * image-wall packer inside a `useMemo` — the packer re-runs whenever the epoch
 * from {@link useQuickPanelImageAspectRatioEpoch} advances, so a non-reactive
 * read here is safe.
 */
export function peekQuickPanelImageAspectRatio(entryId: string): number | undefined {
  return aspectRatioCache.get(entryId)
}

/**
 * Subscribe to *any* aspect-ratio change in the shared cache and get back a
 * monotonically-increasing counter — HistoryPane feeds it into the `useMemo`
 * deps of the image-wall column packer so the packer repacks when a tile
 * publishes a freshly-measured ratio. Cheaper than making the packer subscribe
 * to every id (there is only one packer per pane, and the packer's own read
 * pass will visit whichever ids currently matter).
 */
export function useQuickPanelImageAspectRatioEpoch(): number {
  const [epoch, setEpoch] = useState(0)
  useEffect(() => {
    const listener = () => setEpoch(prev => prev + 1)
    aspectRatioListeners.add(listener)
    return () => {
      aspectRatioListeners.delete(listener)
    }
  }, [])
  return epoch
}
