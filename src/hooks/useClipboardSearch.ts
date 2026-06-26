import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { querySearch, type SearchResultDto } from '@/api/daemon/search'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-clipboard-search')

/**
 * Search executor options. Callers own the query-model construction (debounce,
 * token parsing, filter -> contentTypes mapping); this hook only runs the
 * request once those primitives settle.
 */
export interface UseClipboardSearchOptions {
  /** When false the request is skipped and `results` is null (browse mode). */
  enabled: boolean
  query: string
  /** Backend `contentTypes` param (comma-separated), already resolved. */
  contentTypes?: string
  /** Backend `extensions` param (comma-separated), already resolved. */
  extensions?: string
  /** Backend `sourceDevices` param (comma-separated device ids). */
  sourceDevices?: string
  /** Backend `timePreset` param; omit (undefined) for no time filter. */
  timePreset?: string
  limit?: number
  offset?: number
}

export interface UseClipboardSearchResult<T> {
  /** Null while in browse mode (disabled); an array once a search has run. */
  results: T[] | null
  isSearching: boolean
  /** Total match count reported by the engine, or null in browse mode. */
  total: number | null
}

/**
 * Shared daemon-search pipeline: debounced-input-agnostic request runner with
 * abort, loading state, and a caller-supplied result mapper.
 *
 * This is the single source of truth for *executing* a clipboard search. Both
 * the History page and the quick panel build their own `SearchParams` (from
 * structured controls vs. tokens) but funnel through here so abort/loading
 * semantics and the `querySearch` call never diverge into parallel logic.
 */
export function useClipboardSearch<T>(
  options: UseClipboardSearchOptions,
  mapResult: (dto: SearchResultDto) => T
): UseClipboardSearchResult<T> {
  const { enabled, query, contentTypes, extensions, sourceDevices, timePreset, limit, offset } =
    options
  const [results, setResults] = useState<T[] | null>(null)
  const [isSearching, setIsSearching] = useState(false)
  const [total, setTotal] = useState<number | null>(null)
  const abortRef = useRef<AbortController | null>(null)
  // Mapper identity changes every render (it usually closes over `t`); pin it so
  // it never re-triggers the search effect.
  const mapResultEvent = useEffectEvent(mapResult)

  useEffect(() => {
    abortRef.current?.abort()
    if (!enabled) {
      setResults(null)
      setTotal(null)
      setIsSearching(false)
      return
    }

    const controller = new AbortController()
    abortRef.current = controller
    setIsSearching(true)

    querySearch(
      { query, contentTypes, extensions, sourceDevices, timePreset, limit, offset },
      controller.signal
    )
      .then(response => {
        if (controller.signal.aborted) return
        setResults(response.data.items.map(mapResultEvent))
        setTotal(response.data.total)
        setIsSearching(false)
      })
      .catch(err => {
        if (controller.signal.aborted) return
        if (err instanceof DOMException && err.name === 'AbortError') return
        log.error({ err }, 'Clipboard search failed')
        setResults([])
        setTotal(0)
        setIsSearching(false)
      })

    return () => controller.abort()
  }, [enabled, query, contentTypes, extensions, sourceDevices, timePreset, limit, offset])

  return { results, isSearching, total }
}
