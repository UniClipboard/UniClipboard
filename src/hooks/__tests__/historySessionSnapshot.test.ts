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

  it('drops selection and scroll when the selected entry falls outside the capped window', () => {
    clearHistorySessionSnapshot()

    writeHistorySessionSnapshot({
      searchState: {
        activeFilter: Filter.All,
        searchQuery: '',
        submittedQuery: '',
        tagFilter: null,
        timeRange: 'all_time',
        sourceFilter: null,
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
      // Selected an entry past the cap — it won't survive the window trim.
      selectedId: `entry-${HISTORY_SESSION_ITEM_CAP + 5}`,
      seenIds: [],
      scrollState: { ranges: [], scrollTop: 5000 },
    })

    const snapshot = readHistorySessionSnapshot()
    expect(snapshot?.selectedId).toBeNull()
    expect(snapshot?.scrollState).toBeNull()
  })
})
