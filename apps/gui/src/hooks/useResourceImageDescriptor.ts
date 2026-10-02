import { useEffect, useState } from 'react'
import { getResourceImageUrl } from '@/api/clipboardItems'
import { getClipboardEntryResource } from '@/api/daemon/clipboard'

/**
 * Shared resolver for a clipboard entry's image descriptor: a `data:` URL for
 * inline content, or the daemon blob path for blob-backed content. Bytes are
 * resolved separately (and LRU-bounded) by {@link useBlobImageObjectUrl}.
 *
 * Module-scoped so any number of components resolving the same entry id (a
 * virtualized row, a launcher tile, a thumbnail elsewhere) share one cache and
 * one in-flight daemon request instead of each firing their own.
 */
const descriptorCache = new Map<string, string | null>()
/** entry id -> in-flight lookup, so concurrent callers for the same cold-cache
 * entry share one daemon call instead of racing to fetch it independently. */
const inFlightDescriptorFetches = new Map<string, Promise<string | null>>()

function initialDescriptorState(entryId: string): { entryId: string; descriptor: string | null } {
  return { entryId, descriptor: descriptorCache.get(entryId) ?? null }
}

/**
 * @param entryId Clipboard entry id to resolve.
 * @param enabled When false, skips the daemon lookup entirely (still returns a
 *   cached value if one already exists). Lets callers defer the fetch behind
 *   an explicit condition (e.g. visibility, or a parent that already resolved
 *   the same id and is passing it down instead).
 */
export function useResourceImageDescriptor(entryId: string, enabled = true): string | null {
  const [state, setState] = useState(() => initialDescriptorState(entryId))

  // Row reuse (virtualized list scroll, or quick-panel filter change) can hand
  // this hook a new entry id without remounting the component. Reset
  // synchronously during render — instead of waiting for the effect below,
  // which only runs after paint — so the first frame for the new id never
  // shows the previous entry's cached descriptor.
  if (state.entryId !== entryId) {
    setState(initialDescriptorState(entryId))
  }

  useEffect(() => {
    if (!enabled) return
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
        // Don't cache failures, so a remount (or the next enabled toggle) can retry.
        if (!cancelled) {
          setState(prev => (prev.entryId === entryId ? { ...prev, descriptor: null } : prev))
        }
      })
    return () => {
      cancelled = true
    }
  }, [entryId, enabled])

  return state.descriptor
}

/**
 * Drop the cached descriptor for an entry so the next lookup re-fetches it.
 * Called when an `<img>` errors out (e.g. the blob 404'd because the resource
 * was rewritten) — clears the stale path handed out to any consumer.
 */
export function invalidateResourceImageDescriptor(entryId: string): void {
  descriptorCache.delete(entryId)
}
