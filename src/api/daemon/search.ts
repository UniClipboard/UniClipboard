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
 */

import { daemonClient } from './client'
import type { RequestOptions } from './client'

// ── Response types matching Rust DTOs ──────────────────────────

export interface SearchResultDto {
  entryId: string
  /** Content category (text/html/link/file/image/other). */
  contentType: 'text' | 'html' | 'link' | 'file' | 'image' | 'other'
  activeTimeMs: number
  textPreview: string | null
  mimeType: string
  fileExtensions: string[]
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

// ── Query params ───────────────────────────────────────────────

export interface SearchParams {
  query: string
  /** Content category filter (text, html, link, file, image, other). Sent as `fileTypes` to backend. */
  contentTypes?: string
  extensions?: string
  timePreset?: string
  limit?: number
  offset?: number
}

function buildSearchQueryString(params: SearchParams): string {
  const qs = new URLSearchParams()
  qs.set('query', params.query)
  if (params.contentTypes) qs.set('contentTypes', params.contentTypes)
  if (params.extensions) qs.set('extensions', params.extensions)
  if (params.timePreset) qs.set('timePreset', params.timePreset)
  if (params.limit != null) qs.set('limit', String(params.limit))
  if (params.offset != null) qs.set('offset', String(params.offset))
  return qs.toString()
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
  const qs = buildSearchQueryString(params)
  const options: RequestOptions = { signal }
  return daemonClient.request<SearchQueryResponse>(`/search/query?${qs}`, options)
}

/**
 * Get search index status (ready, rebuilding, unavailable).
 *
 * @returns Current search index status snapshot.
 * @throws {DaemonApiError} On HTTP errors (423 = session locked).
 */
export async function getSearchStatus(): Promise<SearchStatusResponse> {
  return daemonClient.request<SearchStatusResponse>('/search/status')
}

/**
 * Trigger a manual full rebuild of the search index.
 *
 * @throws {DaemonApiError} 409 if rebuild already in progress, 423 if session locked.
 */
export async function triggerSearchRebuild(): Promise<void> {
  await daemonClient.request<unknown>('/search/rebuild', { method: 'POST' })
}
