import {
  buildRelaxationQueries,
  type ChipData,
  type FilterSnapshot,
} from './composite-search-model'
import { type FetchSearchCounts, useSearchCounts } from './useSearchCounts'

export interface Relaxation {
  chip: ChipData
  /** Results the current search would have with only this chip removed. */
  count: number
}

/**
 * When a filtered search comes back empty, count what removing each chip on
 * its own would return, so the empty state can offer the cheapest way out.
 * `null` while inactive, still counting, or when counts are unavailable.
 */
export function useZeroResultRelaxations({
  active,
  chips,
  current,
  query,
  fetchCounts,
}: {
  active: boolean
  chips: ChipData[]
  current: FilterSnapshot
  query: string
  fetchCounts: FetchSearchCounts | undefined
}): Relaxation[] | null {
  const enabled = active && chips.length > 0
  const counts = useSearchCounts(
    enabled ? buildRelaxationQueries(chips, current, query) : null,
    fetchCounts
  )
  if (!enabled || counts === null) return null
  return chips.map((chip, i) => ({ chip, count: counts[i] ?? 0 }))
}
