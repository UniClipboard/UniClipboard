import { HISTORY_WINDOW_CAPACITY } from '@/lib/history-window-policy'
import { createLogger } from '@/lib/logger'
import { daemonClient } from './client'
import { DaemonApiError, DaemonErrorCode } from './errors'

const log = createLogger('blob-image-cache')

/**
 * Upper bound on cached object URLs. Each entry pins the decoded image bytes in
 * memory until revoked, so the cache is LRU-bounded; evicting an entry revokes
 * its object URL. Sized for a virtualized history list's working set.
 */
const MAX_ENTRIES = HISTORY_WINDOW_CAPACITY

/** path -> object URL. Insertion order is the LRU order (Map preserves it). */
const cache = new Map<string, string>()
/** path -> in-flight fetch, so concurrent callers share one request. */
const inFlight = new Map<string, { promise: Promise<string | null>; controller: AbortController }>()
/**
 * Bumped on every {@link clearBlobImageCache}. A fetch started before a teardown
 * carries the generation it began under; if it resolves afterward it revokes its
 * own object URL instead of repopulating the just-cleared cache.
 */
let cacheGeneration = 0
const consumers = new Map<string, number>()

export function retainBlobImage(path: string): void {
  consumers.set(path, (consumers.get(path) ?? 0) + 1)
}

export function releaseBlobImage(path: string): void {
  const count = (consumers.get(path) ?? 1) - 1
  if (count > 0) {
    consumers.set(path, count)
    return
  }
  consumers.delete(path)
  invalidateBlobImageObjectUrl(path)
  // Detach pending work so it cannot populate the cache after its last viewer leaves.
  inFlight.get(path)?.controller.abort()
  inFlight.delete(path)
}

async function fetchAndCache(
  path: string,
  generation: number,
  active: () => boolean,
  signal: AbortSignal
): Promise<string | null> {
  try {
    const blob = await daemonClient.fetchBlob(path, { signal })
    if (generation !== cacheGeneration || !active()) return null
    // Release the oldest URL before allocating another one at capacity.
    if (cache.size >= MAX_ENTRIES && !cache.has(path)) {
      const oldest = cache.keys().next().value
      if (oldest !== undefined) invalidateBlobImageObjectUrl(oldest)
    }
    const objectUrl = URL.createObjectURL(blob)
    // If the cache was cleared while this fetch was in flight, don't
    // resurrect it — revoke the freshly created URL and report no result.
    if (generation !== cacheGeneration || !active()) {
      URL.revokeObjectURL(objectUrl)
      return null
    }
    touch(path, objectUrl)
    return objectUrl
  } catch (err) {
    if (signal.aborted) return null
    if (
      path.endsWith('/thumbnail') &&
      err instanceof DaemonApiError &&
      err.code === DaemonErrorCode.SERVICE_UNAVAILABLE
    ) {
      throw err
    }
    log.error({ err, path }, 'failed to fetch blob image')
    return null
  }
}

function touch(path: string, objectUrl: string): void {
  // Re-insert to move this key to the most-recently-used end.
  cache.delete(path)
  cache.set(path, objectUrl)
  while (cache.size > MAX_ENTRIES) {
    const oldestKey = cache.keys().next().value as string | undefined
    if (oldestKey === undefined) break
    const oldest = cache.get(oldestKey)
    cache.delete(oldestKey)
    if (oldest) URL.revokeObjectURL(oldest)
  }
}

/**
 * Resolve a daemon blob path to a stable `blob:` object URL, fetching the bytes
 * with session auth (and 401 auto-refresh) at most once per path.
 *
 * The object URL carries no token and never expires, so it is safe to keep in a
 * long-lived cache or an already-mounted `<img>`. This replaces embedding a
 * short-lived `?auth=Session <jwt>` token directly in `<img src>`, which caused
 * periodic 401s once the 300s token outlived the cached URL (the browser image
 * loader has no 401-refresh path; see {@link daemonClient.fetchBlob}).
 *
 * @param path Daemon resource path (e.g. "/clipboard/blobs/<id>").
 * @returns A `blob:` object URL, or `null` if the fetch failed.
 */
export async function getBlobImageObjectUrl(path: string): Promise<string | null> {
  const cached = cache.get(path)
  if (cached) {
    touch(path, cached)
    return cached
  }

  const existing = inFlight.get(path)
  if (existing) return existing.promise

  const controller = new AbortController()
  const promise: Promise<string | null> = fetchAndCache(
    path,
    cacheGeneration,
    () => inFlight.get(path)?.promise === promise,
    controller.signal
  ).finally(() => {
    // Only clear the slot we own; a clear()/refetch may have replaced it.
    if (inFlight.get(path)?.promise === promise) inFlight.delete(path)
  })
  inFlight.set(path, { promise, controller })
  return promise
}

/** Read an existing object URL before paint without starting a fetch. */
export function peekBlobImageObjectUrl(path: string): string | null {
  return cache.get(path) ?? null
}

/** Drop a URL that the image decoder rejected. */
export function invalidateBlobImageObjectUrl(path: string): void {
  const url = cache.get(path)
  cache.delete(path)
  if (url) URL.revokeObjectURL(url)
}

/**
 * Revoke every cached object URL and clear the cache. Call on teardown / logout
 * so the pinned image bytes are released.
 */
export function clearBlobImageCache(): void {
  cacheGeneration += 1
  for (const objectUrl of cache.values()) {
    URL.revokeObjectURL(objectUrl)
  }
  cache.clear()
  for (const request of inFlight.values()) request.controller.abort()
  inFlight.clear()
}

/** Read-only E2E diagnostics; unused exports are removed from shipping bundles. */
export const historyImageCacheSize = () => cache.size
export const historyImageCachePaths = () => [...cache.keys()]
