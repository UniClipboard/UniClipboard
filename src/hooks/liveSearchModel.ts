/**
 * Pure model for the unified live browse/search list (Phase 3B).
 *
 * `useLiveSearch` owns a single `DisplayClipboardItem[]` that replaces the split
 * "Redux browse list vs. search results" model: a base query seeds it, realtime
 * `clipboard.new_content` events patch it in place, and user actions
 * (delete / favorite) apply optimistic edits. This module holds the *decision
 * logic* for those patches as pure functions so they can be unit-tested without
 * a daemon or a WebSocket — the realtime path that previously had no automated
 * coverage.
 *
 * ## Live-patch vs. refetch
 * A new entry can only be slotted into the current list client-side when every
 * active filter dimension is judgeable from a `DisplayClipboardItem` alone.
 * Content-type and the `link` / `favorited` tags are; a keyword query (no full
 * text on the item), a source-device filter (no source on the item), a
 * time-range preset (would have to duplicate the backend's preset→range
 * parsing), an extension filter (no extensions on the item), the `code` tag
 * (client derives it from HTML only, but the backend also tags source-like
 * plain text) and the `image` tag (a copied image *file* projects as a `file`
 * item, indistinguishable from a plain file) are not. When any non-judgeable
 * dimension is active,
 * `useLiveSearch` refetches the base query instead of patching (§4.8 fallback).
 */
import type { TimeRangePreset } from '@/api/daemon/search'
import type { ClipboardEntryType, DisplayClipboardItem } from '@/lib/clipboard-entry'

/**
 * Upper bound on entries kept after live prepends; mirrors the retired Redux cap
 * (`clipboardSlice` MAX_LIVE_ITEMS) so a churning clipboard can't grow the list
 * without bound. Pagination (grow-window) legitimately fetches past it and is
 * applied to the base query, not to this prepend path.
 */
export const MAX_LIVE_ITEMS = 200

/**
 * The resolved query the live list is showing. Mirrors the wire `SearchParams`
 * shape (already-resolved comma-separated strings) plus the UI-level `timeRange`
 * preset (kept as the preset, since the backend owns preset→range parsing).
 */
export interface LiveSearchQueryModel {
  /** Keyword query; an empty string means browse. */
  query: string
  /** Backend `contentTypes` param (comma-separated physical categories). */
  contentTypes?: string
  /** Backend `tags` param (comma-separated tag ids, e.g. `link`, `favorited`). */
  tags?: string
  /** Backend `sourceDevices` param (comma-separated device ids). */
  sourceDevices?: string
  /** Backend `extensions` param (comma-separated). */
  extensions?: string
  /** Time-range preset; `all_time`/undefined means no time filter. */
  timeRange?: TimeRangePreset
}

/**
 * Collapse a display type back to the backend physical content category so a
 * content-type filter (which narrows physical categories) can be matched
 * against a projected item. `link` is a `text` entry that carries web URLs, so
 * it maps back to `text`.
 */
export function displayTypeToContentType(type: ClipboardEntryType): string {
  switch (type) {
    case 'text':
    case 'link':
      return 'text'
    case 'code':
      return 'html'
    case 'file':
      return 'file'
    case 'image':
      return 'image'
    case 'unknown':
      return 'other'
  }
}

/**
 * Builtin tags whose membership is reliably derivable from a
 * `DisplayClipboardItem` alone: `link` (a content tag) and `favorited` (its
 * flag). `code` is deliberately absent — local inserts derive `code` only from
 * HTML (`projectClipboardEntry`), but the backend also tags source-like plain
 * text, so a freshly-arrived plain-text entry the server would tag `code`
 * arrives untagged and would be wrongly dropped from an active `#code` view.
 * `image` is absent for the same reason — a copied image *file* projects as a
 * `file` display item, indistinguishable from a plain file client-side, so the
 * server must decide. Custom tags are likewise non-judgeable.
 */
const CLIENT_JUDGEABLE_TAGS = new Set(['link', 'favorited'])

/**
 * Whether a freshly-arrived entry can be slotted into the current list purely
 * client-side. False when a keyword query, source, time-range, or extension
 * filter is active, or when a non-client-judgeable tag (`image` / custom) is
 * active (none judgeable from a `DisplayClipboardItem`) — the caller refetches
 * the base query instead.
 */
export function canPatchLive(model: LiveSearchQueryModel): boolean {
  const tagsJudgeable = splitCsv(model.tags).every(tag => CLIENT_JUDGEABLE_TAGS.has(tag))
  return (
    model.query.trim().length === 0 &&
    !model.sourceDevices &&
    !model.extensions &&
    tagsJudgeable &&
    (model.timeRange === undefined || model.timeRange === 'all_time')
  )
}

/**
 * Payload of a `search` topic status event (`search.status_snapshot`). The
 * daemon reports the index availability under `state` (`'ready'` /
 * `'rebuilding'` / `'unavailable'`); older daemon builds emitted incremental
 * updates under `status`, so both keys are read for version tolerance.
 */
export interface SearchStatusEventPayload {
  state?: string
  status?: string
  reason?: string | null
}

/**
 * Whether a search-index status event should trigger a refetch of the current
 * window. The degraded browse banner is driven by the last query's `state`; a
 * filter-less browse served while the index rebuilds keeps patching in new
 * entries client-side and never re-queries, so the banner persists until the
 * index becomes ready *and* we re-issue the query. Returns true exactly when the
 * index just reported `ready` while the current view is still `degraded` — the
 * refetch then upgrades it to the index-backed result and clears the banner.
 */
export function shouldRefetchOnSearchStatus(
  payload: SearchStatusEventPayload | undefined,
  currentState: 'ready' | 'degraded'
): boolean {
  const indexStatus = payload?.state ?? payload?.status
  return indexStatus === 'ready' && currentState === 'degraded'
}

function splitCsv(value: string | undefined): string[] {
  if (!value) return []
  return value
    .split(',')
    .map(part => part.trim())
    .filter(Boolean)
}

function tagMatches(tag: string, item: DisplayClipboardItem): boolean {
  if (tag === 'link') return item.contentTags?.includes('link') || item.type === 'link'
  if (tag === 'code') return item.contentTags?.includes('code') || item.type === 'code'
  if (tag === 'favorited') return item.isFavorited === true
  // Custom tags aren't derivable from a DisplayItem; treat as non-matching so a
  // new entry is never optimistically shown under a tag it may not carry.
  return false
}

/**
 * Whether `item` satisfies the content-type and tag dimensions of `model`.
 * Only meaningful when {@link canPatchLive} is true (the other dimensions are
 * excluded there). Multiple content-types/tags are OR-ed within their
 * dimension, matching the engine's `eq_any` semantics.
 */
export function matchesFilter(item: DisplayClipboardItem, model: LiveSearchQueryModel): boolean {
  const contentTypes = splitCsv(model.contentTypes)
  if (contentTypes.length > 0 && !contentTypes.includes(displayTypeToContentType(item.type))) {
    return false
  }
  const tags = splitCsv(model.tags)
  if (tags.length > 0 && !tags.some(tag => tagMatches(tag, item))) {
    return false
  }
  return true
}

/**
 * Prepend a freshly-arrived entry, de-duplicating by id (a re-copy of an entry
 * already present floats back to the front with its latest projection) and
 * trimming the oldest tail past `cap`.
 */
export function prependLiveItem(
  items: DisplayClipboardItem[],
  incoming: DisplayClipboardItem,
  cap: number = MAX_LIVE_ITEMS
): DisplayClipboardItem[] {
  const next = [incoming, ...items.filter(it => it.id !== incoming.id)]
  return next.length > cap ? next.slice(0, cap) : next
}

/**
 * Drop an entry by id. Returns the same array reference when the id is absent so
 * React can skip a re-render.
 */
export function removeLiveItem(items: DisplayClipboardItem[], id: string): DisplayClipboardItem[] {
  const next = items.filter(it => it.id !== id)
  return next.length === items.length ? items : next
}

/**
 * Merge a partial patch into the entry with `id` (favorite toggle, payload
 * lost). Returns the same array reference when the id is absent.
 */
export function patchLiveItem(
  items: DisplayClipboardItem[],
  id: string,
  patch: Partial<DisplayClipboardItem>
): DisplayClipboardItem[] {
  let changed = false
  const next = items.map(it => {
    if (it.id !== id) return it
    changed = true
    return { ...it, ...patch }
  })
  return changed ? next : items
}
