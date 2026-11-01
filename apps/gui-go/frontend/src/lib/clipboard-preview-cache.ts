import { loadClipboardPreview } from '@/lib/clipboard-preview-loader'

export interface ClipboardPreviewData {
  entryId: string
  contentType: 'text' | 'image' | 'file'
  sizeBytes: number
  textContent?: string
  /**
   * Token-free image descriptor: a `data:` URL (inline content) or a daemon
   * blob path. Consumers resolve it to a `blob:` object URL at `<img>` mount
   * time via `useBlobImageObjectUrl`; storing a path (not a token-bearing URL)
   * keeps this cacheable without the 300s-token 401 churn.
   */
  imageBlobPath?: string
  /** Image MIME type from the entry's resource (e.g. `image/png`). */
  mimeType?: string
  fileNames?: string[]
  /**
   * Image preview exceeds the auto-inline threshold (D6 / ADR-008 P3-d): the
   * original must not be auto-pulled. `imageBlobPath` still carries the
   * descriptor, but consumers must gate the actual `<img>` mount (and its blob
   * fetch) behind an explicit user action so the daemon only materializes the
   * large payload on demand. See `INLINE_PREVIEW_MAX_BYTES`.
   */
  requiresExplicitLoad?: boolean
}

interface CacheRecord {
  expiresAt: number
  value: ClipboardPreviewData
}

const DEFAULT_TTL_MS = 30_000

class ClipboardPreviewCache {
  private ttlMs: number
  private cache = new Map<string, CacheRecord>()
  private inFlight = new Map<string, Promise<ClipboardPreviewData | null>>()

  constructor(ttlMs: number = DEFAULT_TTL_MS) {
    this.ttlMs = ttlMs
  }

  /** Read a fresh preview synchronously so revisiting an entry does not paint a loading frame. */
  peek(entryId: string): ClipboardPreviewData | null {
    const cached = this.cache.get(entryId)
    return cached && cached.expiresAt > Date.now() ? cached.value : null
  }

  async get(entryId: string): Promise<ClipboardPreviewData | null> {
    const now = Date.now()
    const cached = this.cache.get(entryId)
    if (cached && cached.expiresAt > now) {
      return cached.value
    }
    if (cached) {
      this.cache.delete(entryId)
    }

    const existingPromise = this.inFlight.get(entryId)
    if (existingPromise) {
      return existingPromise
    }

    const promise = loadClipboardPreview(entryId)
    this.inFlight.set(entryId, promise)

    try {
      const value = await promise
      // Compare request identity, not its awaited value: invalidation may have replaced the slot.
      if (value && this.inFlight.get(entryId) === promise) {
        this.cache.set(entryId, {
          value,
          expiresAt: Date.now() + this.ttlMs,
        })
      }
      return value
    } finally {
      // An obsolete request must not remove a newer pending request.
      if (this.inFlight.get(entryId) === promise) this.inFlight.delete(entryId)
    }
  }

  invalidate(entryId: string): void {
    this.cache.delete(entryId)
    this.inFlight.delete(entryId)
  }

  retain(ids: ReadonlySet<string>, selectedId: string | null = null): void {
    for (const id of this.cache.keys()) if (id !== selectedId && !ids.has(id)) this.cache.delete(id)
    for (const id of this.inFlight.keys())
      if (id !== selectedId && !ids.has(id)) this.inFlight.delete(id)
  }

  clear(): void {
    this.cache.clear()
    this.inFlight.clear()
  }
}

export const clipboardPreviewCache = new ClipboardPreviewCache()
