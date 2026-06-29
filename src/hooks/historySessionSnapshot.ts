import type { StateSnapshot } from 'react-virtuoso'
import type { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import type { LiveSearchQueryModel } from '@/hooks/liveSearchModel'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

export const HISTORY_SESSION_ITEM_CAP = 100

export interface HistorySearchStateSnapshot {
  activeFilter: Filter
  searchQuery: string
  submittedQuery: string
  tagFilter: string | null
  timeRange: TimeRangePreset
  sourceFilter: string | null
}

export interface HistoryLiveSnapshot {
  model: LiveSearchQueryModel
  items: DisplayClipboardItem[]
  total: number | null
  hasMore: boolean
  state: 'ready' | 'degraded'
}

export interface HistorySessionSnapshot {
  searchState: HistorySearchStateSnapshot
  live: HistoryLiveSnapshot | null
  selectedId: string | null
  seenIds: string[]
  scrollState: StateSnapshot | null
}

let snapshot: HistorySessionSnapshot | null = null

export function readHistorySessionSnapshot(): HistorySessionSnapshot | null {
  return snapshot
}

export function writeHistorySessionSnapshot(next: HistorySessionSnapshot): void {
  const cappedItems = next.live?.items.slice(0, HISTORY_SESSION_ITEM_CAP) ?? []
  // A selection that points past the capped window can't be restored: the
  // restore would clear the missing selection, auto-select the first row, and
  // the saved scroll offset would no longer anchor on the right entry. Drop the
  // selection and scroll together in that case so restore starts consistent.
  const selectionInWindow =
    next.selectedId === null || cappedItems.some(it => it.id === next.selectedId)
  snapshot = {
    ...next,
    live: next.live ? { ...next.live, items: cappedItems } : null,
    seenIds: next.seenIds.slice(0, HISTORY_SESSION_ITEM_CAP),
    selectedId: selectionInWindow ? next.selectedId : null,
    scrollState: selectionInWindow ? next.scrollState : null,
  }
}

export function updateHistorySessionSelection(selectedId: string | null): void {
  if (!snapshot) return
  snapshot = {
    ...snapshot,
    selectedId,
  }
}

export function clearHistorySessionSnapshot(): void {
  snapshot = null
}
