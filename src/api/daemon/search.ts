/**
 * Daemon search API module — typed HTTP client for the search endpoints.
 *
 * # Endpoints
 * - `GET /search/query` → execute a structured search query
 * - `GET /search/status` → search index availability snapshot
 * - `POST /search/rebuild` → trigger manual full rebuild
 *
 * # Type alignment
 * Frontend types are kept identical to backend query params so no mapping
 * layer is needed:
 * - `TimeRangePreset` values match backend `timePreset` param values directly.
 * - `Filter` enum values match backend `fileTypes` param values directly
 *   (except `Filter.All` / `Filter.Favorited` which are omitted).
 *
 * # Transport / 传输 (ADR-008 P7)
 * All three endpoints route through the @hey-api generated SDK
 * (`searchQuery` / `getSearchStatus` / `rebuildSearchIndex`) via
 * `daemonClient.callSdk`, which drives the daemon session lifecycle and
 * normalizes SDK errors back into `DaemonApiError`. `callSdk` unwraps the SDK's
 * outer `{ data }` to the canonical `ApiEnvelope { data, ts }`. The public
 * wrappers preserve their exact return shapes (the query/status wrappers return
 * the envelope so consumers keep reading `.data`).
 */

import {
  getSearchTags as getSearchTagsSdk,
  searchQuery,
  getSearchStatus as getSearchStatusSdk,
  rebuildSearchIndex,
} from '@/api/generated/sdk.gen'
import { daemonClient } from './client'

// ── Response types matching Rust DTOs ──────────────────────────

export interface SearchResultDto {
  entryId: string
  /** Physical content category (text/html/file/image/other). `link` is a tag. */
  contentType: 'text' | 'html' | 'file' | 'image' | 'other'
  activeTimeMs: number
  /** Derived/user-state tag ids (e.g. 'link', 'favorited'). */
  tags: string[]
  textPreview: string | null
  /**
   * Full character count of the entry's primary text content. `textPreview` is
   * capped at 200 chars, so this carries the real total length for the card's
   * size label. `null` for entries with no inline text (image / file / lost).
   */
  charCount: number | null
  mimeType: string
  fileExtensions: string[]
  /** Display names of referenced files; empty when none. */
  fileNames: string[]
  /** Web URLs (http/https) carried by this entry; empty when none. */
  linkUrls: string[]
  /** Originating device id, or null when the source is unknown. */
  sourceDevice: string | null
  /** 'Lost' when the paste payload is unrecoverable, else null. */
  payloadState: string | null
}

/**
 * Folded payload for `GET /search/query` (ADR-008 §0.1).
 *
 * `total`/`hasMore` are no longer top-level siblings of the envelope — they are
 * folded INTO the `data` payload alongside the renamed `items` array. The
 * endpoint now returns the canonical `ApiEnvelope<SearchQueryResultDto>`.
 */
export interface SearchQueryResultDto {
  items: SearchResultDto[]
  total: number
  hasMore: boolean
  /**
   * `'ready'` when served from the index, or `'degraded'` when the index was
   * not ready and this filter-less browse was served from the main store
   * (§4.7). Filtered/keyword queries surface an `index_rebuilding` error instead.
   */
  state: 'ready' | 'degraded'
}

export interface SearchQueryResponse {
  data: SearchQueryResultDto
  ts: number
}

export interface SearchStatusData {
  state: 'ready' | 'rebuilding' | 'unavailable'
  reason: string | null
  lastRebuildStartedAtMs: number | null
  lastRebuildCompletedAtMs: number | null
}

export interface SearchStatusResponse {
  data: SearchStatusData
  ts: number
}

export interface SearchTagDto {
  tagId: string
  count: number
  isBuiltin: boolean
}

export interface SearchTagsResponse {
  data: SearchTagDto[]
  ts: number
}

// ── Query params ───────────────────────────────────────────────

/**
 * Time-range presets. Values match the backend `timePreset` query param
 * directly; `all_time` is a UI-only sentinel meaning "no time filter" and must
 * be omitted from the wire (see `querySearch`).
 */
export type TimeRangePreset =
  | 'all_time'
  | 'today'
  | 'yesterday'
  | 'last_7d'
  | 'last_30d'
  | 'this_week'
  | 'this_month'

export interface SearchParams {
  query: string
  /** Content category filter (text, html, file, image, other). */
  contentTypes?: string
  /** Comma-separated tag ids (e.g. 'link', 'favorited'). Custom tags require an
   * unlocked session. */
  tags?: string
  extensions?: string
  /** Comma-separated source device ids; restricts results to those origins. */
  sourceDevices?: string
  timePreset?: string
  limit?: number
  offset?: number
}

// ── API functions ──────────────────────────────────────────────

/**
 * Execute a search query against the daemon search index.
 *
 * @param params Search parameters — values should match backend param format directly.
 * @param signal Optional AbortSignal for request cancellation.
 * @returns Search results with total count and pagination info.
 * @throws {DaemonApiError} On HTTP errors (423 = session locked, 400 = invalid query).
 */
export async function querySearch(
  params: SearchParams,
  signal?: AbortSignal
): Promise<SearchQueryResponse> {
  // Only emit query keys that are actually present, preserving the previous
  // "undefined/empty == no filter" wire semantics (the old query-string builder
  // skipped falsy optional params).
  const query: { query: string; [key: string]: string | number } = {
    query: params.query,
  }
  if (params.contentTypes) query.contentTypes = params.contentTypes
  if (params.tags) query.tags = params.tags
  if (params.extensions) query.extensions = params.extensions
  if (params.sourceDevices) query.sourceDevices = params.sourceDevices
  if (params.timePreset) query.timePreset = params.timePreset
  if (params.limit != null) query.limit = params.limit
  if (params.offset != null) query.offset = params.offset

  // `callSdk` unwraps the SDK's `{ data }` to the canonical envelope
  // (`SearchQueryEnvelope`), which is structurally equivalent to the
  // hand-written `SearchQueryResponse`. Consumers read `.data`, so the envelope
  // is returned as-is, bridged to keep the public return type stable.
  const envelope = await daemonClient.callSdk(() =>
    searchQuery({ query, signal, throwOnError: true })
  )
  return envelope as unknown as SearchQueryResponse
}

/**
 * Get search index status (ready, rebuilding, unavailable).
 *
 * @returns Current search index status snapshot.
 * @throws {DaemonApiError} On HTTP errors (423 = session locked).
 */
export async function getSearchStatus(): Promise<SearchStatusResponse> {
  // `callSdk` unwraps the SDK's `{ data }` to the canonical envelope
  // (`SearchStatusEnvelope`); consumers read `.data`, so return the envelope
  // as-is, bridged to the hand-written `SearchStatusResponse` shape.
  const envelope = await daemonClient.callSdk(() => getSearchStatusSdk({ throwOnError: true }))
  return envelope as unknown as SearchStatusResponse
}

/**
 * List searchable tag ids present in the index.
 */
export async function getSearchTags(): Promise<SearchTagsResponse> {
  const envelope = await daemonClient.callSdk(() => getSearchTagsSdk({ throwOnError: true }))
  return envelope as unknown as SearchTagsResponse
}

/**
 * Trigger a manual full rebuild of the search index.
 *
 * @throws {DaemonApiError} 409 if rebuild already in progress, 423 if session locked.
 */
export async function triggerSearchRebuild(): Promise<void> {
  // Void endpoint: drive the rebuild and ignore the acceptance payload.
  await daemonClient.callSdk(() => rebuildSearchIndex({ throwOnError: true }))
}
