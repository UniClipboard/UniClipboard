import { useEffect, useState } from 'react'
import type { SearchParams } from '@/api/daemon/search'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-search-counts')

/**
 * Batch count fetcher; resolves to counts in input order. Pass a stable
 * reference: a new function each render restarts the debounce forever.
 */
export type FetchSearchCounts = (queries: SearchParams[], signal: AbortSignal) => Promise<number[]>

const DEBOUNCE_MS = 250

/**
 * Debounced, latest-wins batch counts for `queries` (`null` = don't fetch).
 *
 * Counts are hints: any failure (index rebuilding, session locked, …) yields
 * `null` and nothing is surfaced to the user. Without `fetchCounts` the hook
 * never touches the network.
 */
export function useSearchCounts(
  queries: SearchParams[] | null,
  fetchCounts: FetchSearchCounts | undefined
): number[] | null {
  const key = queries && queries.length > 0 && fetchCounts ? JSON.stringify(queries) : null
  const [result, setResult] = useState<{ key: string; counts: number[] } | null>(null)

  useEffect(() => {
    if (key === null || !fetchCounts) return
    const controller = new AbortController()
    const timer = setTimeout(() => {
      fetchCounts(JSON.parse(key) as SearchParams[], controller.signal)
        .then(counts => {
          if (!controller.signal.aborted) setResult({ key, counts })
        })
        .catch(err => {
          if (!controller.signal.aborted) log.debug({ err }, 'search counts unavailable')
        })
    }, DEBOUNCE_MS)
    return () => {
      clearTimeout(timer)
      controller.abort()
    }
  }, [key, fetchCounts])

  return result !== null && result.key === key ? result.counts : null
}
