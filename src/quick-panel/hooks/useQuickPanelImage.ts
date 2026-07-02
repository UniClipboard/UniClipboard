import { useEffect, useState } from 'react'
import { getResourceImageUrl } from '@/api/clipboardItems'
import { getClipboardEntryResource } from '@/api/daemon/clipboard'
import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'

/**
 * Quick-panel image resolver.
 *
 * Two caches keyed by entry id, both module-scoped so scrolling the launcher
 * back to a row (or briefly closing and reopening the panel — this file lives
 * alongside the panel module, so the cache survives HistoryPane remounts) does
 * NOT re-hit the daemon and does NOT re-flicker aspect ratios in the image
 * wall. A third map, `inFlightDescriptorFetches`, dedupes the daemon lookup
 * itself when multiple hook instances (e.g. an `ImageGridItem` tile and the
 * `QuickPanelImage` it renders) resolve the same cold-cache entry id at once:
 *
 * - `descriptorCache` — the token-free image descriptor: a `data:` URL for
 *   inline content or the daemon blob path for blob-backed content. The actual
 *   bytes are cached separately (LRU-bounded) by {@link useBlobImageObjectUrl},
 *   which also strips the short-lived session token from the rendered
 *   `<img src>`.
 * - `aspectRatioCache` — the intrinsic aspect ratio measured on `<img.onLoad>`
 *   the first time we saw this entry. The daemon does carry image dimensions
 *   on the entry (`imageWidth`/`imageHeight`) and on the search DTO, but the
 *   quick panel's `DisplayItem` projection currently drops them; caching the
 *   measured value here is enough to keep the masonry layout stable on
 *   subsequent renders without threading dimensions through the projection.
 */
const descriptorCache = new Map<string, string | null>()
const aspectRatioCache = new Map<string, number>()
/** entry id -> in-flight descriptor lookup, so concurrent hook instances (e.g.
 * `ImageGridItem` and the `QuickPanelImage` it renders) share one daemon call
 * instead of both racing to fetch the same entry's resource. */
const inFlightDescriptorFetches = new Map<string, Promise<string | null>>()

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

function initialQuickPanelImageState(entryId: string): {
  entryId: string
  descriptor: string | null
  aspectRatio: number | undefined
} {
  return {
    entryId,
    descriptor: descriptorCache.get(entryId) ?? null,
    aspectRatio: aspectRatioCache.get(entryId),
  }
}

export function useQuickPanelImage(entryId: string): QuickPanelImage {
  const [state, setState] = useState(() => initialQuickPanelImageState(entryId))

  // Row reuse (filter change) can hand this hook a new entry id without
  // remounting the component. Reset synchronously during render — instead of
  // waiting for the effects below, which only run after paint — so the first
  // frame for the new id never shows the previous entry's cached
  // descriptor/aspect ratio.
  if (state.entryId !== entryId) {
    setState(initialQuickPanelImageState(entryId))
  }
  const { descriptor, aspectRatio } = state

  // Descriptor lookup: use cache, else hit the daemon once (deduped across
  // concurrent callers) and memoize.
  useEffect(() => {
    const cached = descriptorCache.get(entryId)
    if (cached !== undefined) {
      setState(prev => (prev.entryId === entryId ? { ...prev, descriptor: cached } : prev))
      return
    }
    let cancelled = false

    let fetchPromise = inFlightDescriptorFetches.get(entryId)
    if (!fetchPromise) {
      fetchPromise = getClipboardEntryResource(entryId)
        .then(resource => (resource ? getResourceImageUrl(resource) : null))
        .finally(() => {
          inFlightDescriptorFetches.delete(entryId)
        })
      inFlightDescriptorFetches.set(entryId, fetchPromise)
    }

    fetchPromise
      .then(next => {
        if (cancelled) return
        descriptorCache.set(entryId, next)
        setState(prev => (prev.entryId === entryId ? { ...prev, descriptor: next } : prev))
      })
      .catch(() => {
        // Don't cache failures, so a remount can retry.
        if (!cancelled) {
          setState(prev => (prev.entryId === entryId ? { ...prev, descriptor: null } : prev))
        }
      })
    return () => {
      cancelled = true
    }
  }, [entryId])

  // Aspect-ratio subscription: pull the cached value on mount (in case another
  // instance measured it first) and re-render whenever any writer publishes a
  // new value for this entry.
  useEffect(() => {
    setState(prev =>
      prev.entryId === entryId ? { ...prev, aspectRatio: aspectRatioCache.get(entryId) } : prev
    )
    const listener = () =>
      setState(prev =>
        prev.entryId === entryId ? { ...prev, aspectRatio: aspectRatioCache.get(entryId) } : prev
      )
    aspectRatioListeners.add(listener)
    return () => {
      aspectRatioListeners.delete(listener)
    }
  }, [entryId])

  const url = useBlobImageObjectUrl(descriptor)
  return { url, aspectRatio }
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

/**
 * Drop the cached descriptor for an entry so the next render re-fetches it.
 * Called when the `<img>` errors out (e.g. the blob 404'd because the resource
 * was rewritten) — clears the stale path we handed out. The aspect ratio is
 * left in place: image dimensions don't change even if the bytes go missing.
 */
export function invalidateQuickPanelImageUrl(entryId: string): void {
  descriptorCache.delete(entryId)
}
