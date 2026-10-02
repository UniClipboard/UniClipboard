import { createLogger } from '@/lib/logger'
import { daemonClient } from './client'

const log = createLogger('blob-image-cache')

/**
 * Upper bound on cached object URLs. Each entry pins the decoded image bytes in
 * memory until revoked, so the cache is LRU-bounded; evicting an entry revokes
 * its object URL. Sized for a virtualized history list's working set.
 */
const MAX_ENTRIES = 128

/** path -> object URL. Insertion order is the LRU order (Map preserves it). */
const cache = new Map<string, string>()
/** path -> in-flight fetch, so concurrent callers share one request. */
const inFlight = new Map<string, Promise<string | null>>()
/**
 * Bumped on every {@link clearBlobImageCache}. A fetch started before a teardown
 * carries the generation it began under; if it resolves afterward it revokes its
 * own object URL instead of repopulating the just-cleared cache.
 */
let cacheGeneration = 0

async function fetchAndCache(path: string, generation: number): Promise<string | null> {
  try {
    const blob = await daemonClient.fetchBlob(path)
    const objectUrl = URL.createObjectURL(blob)
    // If the cache was cleared while this fetch was in flight, don't
    // resurrect it — revoke the freshly created URL and report no result.
    if (generation !== cacheGeneration) {
      URL.revokeObjectURL(objectUrl)
      return null
    }
    touch(path, objectUrl)
    return objectUrl
  } catch (err) {
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
  if (existing) return existing

  const promise = fetchAndCache(path, cacheGeneration).finally(() => {
    // Only clear the slot we own; a clear()/refetch may have replaced it.
    if (inFlight.get(path) === promise) inFlight.delete(path)
  })
  inFlight.set(path, promise)
  return promise
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
  inFlight.clear()
}
