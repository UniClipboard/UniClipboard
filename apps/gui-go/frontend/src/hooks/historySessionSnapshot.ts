import type { StateSnapshot } from 'react-virtuoso'
import type { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import type { LiveSearchQueryModel } from '@/hooks/liveSearchModel'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { HISTORY_WINDOW_CAPACITY } from '@/lib/history-window-policy'

export const HISTORY_SESSION_ITEM_CAP = HISTORY_WINDOW_CAPACITY

export interface HistorySearchStateSnapshot {
  activeFilter: Filter
  searchQuery: string
  submittedQuery: string
  tagFilter: string | null
  timeRange: TimeRangePreset
  sourceFilter: string | null
  extensionFilter: string | null
}

export interface HistoryLiveSnapshot {
  offset?: number
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
  selectedItem?: DisplayClipboardItem | null
  selectedIndex?: number | null
  seenIds: string[]
  scrollState: StateSnapshot | null
}

let snapshot: HistorySessionSnapshot | null = null

export function readHistorySessionSnapshot(): HistorySessionSnapshot | null {
  return snapshot
}

export function writeHistorySessionSnapshot(next: HistorySessionSnapshot): void {
  const cappedItems = next.live?.items.slice(0, HISTORY_SESSION_ITEM_CAP) ?? []
  const selectedLocalIndex = next.live?.items.findIndex(item => item.id === next.selectedId) ?? -1
  const selectedItem =
    next.selectedId === null
      ? null
      : selectedLocalIndex >= 0
        ? next.live!.items[selectedLocalIndex]
        : next.selectedItem?.id === next.selectedId
          ? next.selectedItem
          : null
  const selectedIndex =
    next.selectedId === null
      ? null
      : (next.selectedIndex ??
        (selectedLocalIndex >= 0 ? (next.live?.offset ?? 0) + selectedLocalIndex : null))
  snapshot = {
    ...next,
    live: next.live ? { ...next.live, items: cappedItems } : null,
    seenIds: next.seenIds.slice(0, HISTORY_SESSION_ITEM_CAP),
    selectedItem,
    selectedIndex,
  }
}

export function updateHistorySessionSelection(
  selectedId: string | null,
  selectedItem: DisplayClipboardItem | null = null,
  selectedIndex: number | null = null
): void {
  if (!snapshot) return
  snapshot = {
    ...snapshot,
    selectedId,
    selectedItem,
    selectedIndex,
  }
}

export function clearHistorySessionSnapshot(): void {
  snapshot = null
}
