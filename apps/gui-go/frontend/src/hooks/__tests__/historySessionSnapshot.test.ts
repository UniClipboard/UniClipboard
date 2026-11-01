import { describe, expect, it } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import {
  HISTORY_SESSION_ITEM_CAP,
  clearHistorySessionSnapshot,
  readHistorySessionSnapshot,
  writeHistorySessionSnapshot,
} from '../historySessionSnapshot'

function makeItem(index: number): DisplayClipboardItem {
  return {
    id: `entry-${index}`,
    type: 'text',
    content: {
      display_text: `Clipboard entry ${index}`,
      has_detail: false,
      size: index,
    },
    activeTime: index,
  }
}

describe('historySessionSnapshot', () => {
  it('keeps only a capped lightweight history snapshot', () => {
    clearHistorySessionSnapshot()

    writeHistorySessionSnapshot({
      searchState: {
        activeFilter: Filter.All,
        searchQuery: '',
        submittedQuery: '',
        tagFilter: null,
        timeRange: 'all_time',
        sourceFilter: null,
        extensionFilter: null,
      },
      live: {
        model: { query: '' },
        items: Array.from({ length: HISTORY_SESSION_ITEM_CAP + 20 }, (_value, index) =>
          makeItem(index)
        ),
        total: HISTORY_SESSION_ITEM_CAP + 20,
        hasMore: true,
        state: 'ready',
      },
      selectedId: 'entry-2',
      seenIds: Array.from(
        { length: HISTORY_SESSION_ITEM_CAP + 20 },
        (_value, index) => `entry-${index}`
      ),
      scrollState: { ranges: [], scrollTop: 120 },
    })

    const snapshot = readHistorySessionSnapshot()
    const live = snapshot?.live
    expect(live).not.toBeNull()
    expect(live?.items).toHaveLength(HISTORY_SESSION_ITEM_CAP)
    expect(live?.items.at(-1)?.id).toBe(`entry-${HISTORY_SESSION_ITEM_CAP - 1}`)
    expect(snapshot?.seenIds).toHaveLength(HISTORY_SESSION_ITEM_CAP)
    expect(snapshot?.scrollState?.scrollTop).toBe(120)
  })

  it('keeps one selected projection and scroll outside the capped window', () => {
    clearHistorySessionSnapshot()

    writeHistorySessionSnapshot({
      searchState: {
        activeFilter: Filter.All,
        searchQuery: '',
        submittedQuery: '',
        tagFilter: null,
        timeRange: 'all_time',
        sourceFilter: null,
        extensionFilter: null,
      },
      live: {
        model: { query: '' },
        items: Array.from({ length: HISTORY_SESSION_ITEM_CAP + 20 }, (_value, index) =>
          makeItem(index)
        ),
        total: HISTORY_SESSION_ITEM_CAP + 20,
        hasMore: true,
        state: 'ready',
      },
      // The separate projection survives even when the record window is capped.
      selectedId: `entry-${HISTORY_SESSION_ITEM_CAP + 5}`,
      seenIds: [],
      scrollState: { ranges: [], scrollTop: 5000 },
    })

    const snapshot = readHistorySessionSnapshot()
    expect(snapshot?.live?.items).toHaveLength(50)
    expect(snapshot?.selectedId).toBe('entry-55')
    expect(snapshot?.selectedItem?.id).toBe('entry-55')
    expect(snapshot?.selectedIndex).toBe(55)
    expect(snapshot?.scrollState?.scrollTop).toBe(5000)
    writeHistorySessionSnapshot({
      ...snapshot!,
      live: { ...snapshot!.live!, items: [makeItem(0)] },
    })
    expect(readHistorySessionSnapshot()?.selectedItem?.id).toBe('entry-55')
  })
})
