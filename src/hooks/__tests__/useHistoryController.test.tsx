import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import { useShortcut } from '@/hooks/useShortcut'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import {
  clearHistorySessionSnapshot,
  readHistorySessionSnapshot,
  writeHistorySessionSnapshot,
} from '../historySessionSnapshot'
import { useHistoryController } from '../useHistoryController'

const clipboardItemsApi = vi.hoisted(() => ({
  favoriteClipboardItem: vi.fn(() => Promise.resolve(true)),
  unfavoriteClipboardItem: vi.fn(() => Promise.resolve(true)),
}))

const historyDataState = vi.hoisted(() => ({
  baseItems: [] as DisplayClipboardItem[],
}))

const items: DisplayClipboardItem[] = [
  {
    id: 'entry-1',
    type: 'text',
    content: { display_text: 'first', has_detail: false, size: 5 },
    activeTime: 1,
  },
  {
    id: 'entry-2',
    type: 'text',
    content: { display_text: 'second', has_detail: false, size: 6 },
    activeTime: 2,
  },
]

vi.mock('@/api/clipboardItems', async importOriginal => {
  const actual = await importOriginal<typeof import('@/api/clipboardItems')>()
  return {
    ...actual,
    favoriteClipboardItem: clipboardItemsApi.favoriteClipboardItem,
    unfavoriteClipboardItem: clipboardItemsApi.unfavoriteClipboardItem,
  }
})

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/hooks/useHistoryData', () => ({
  useHistoryData: () => ({
    filter: {
      activeFilter: Filter.All,
      searchQuery: '',
      submittedQuery: '',
      tagFilter: null,
      timeRange: 'all_time',
      sourceFilter: null,
    },
    actions: {
      setContentFilter: vi.fn(),
      setTagFilter: vi.fn(),
      setSourceFilter: vi.fn(),
      setTimeRange: vi.fn(),
      setQuery: vi.fn(),
      submitQuery: vi.fn(),
    },
    sourceOptions: [],
    baseItems: historyDataState.baseItems,
    liveSnapshot: {
      model: { query: '', timeRange: 'all_time' },
      items: historyDataState.baseItems,
      total: historyDataState.baseItems.length,
      hasMore: false,
      state: 'ready',
    },
    browseCount: historyDataState.baseItems.length,
    indexState: 'ready',
    isSearchActive: false,
    searchLoading: false,
    hasMore: false,
    handleLoadMore: vi.fn(),
    removeItem: vi.fn(),
  }),
}))

vi.mock('@/hooks/useSearchTags', () => ({
  useSearchTags: () => [],
}))

vi.mock('@/hooks/useShortcut', () => ({ useShortcut: vi.fn() }))

vi.mock('@/hooks/useShortcutScope', () => ({
  useShortcutScope: vi.fn(),
}))

vi.mock('@/hooks/useTransferProgress', () => ({
  useTransferProgress: vi.fn(),
}))

vi.mock('@/hooks/useCopyFeedback', () => ({
  useCopyFeedback: () => ({
    copySuccessId: null,
    promotedId: null,
    markCopied: vi.fn(),
  }),
}))

vi.mock('@/hooks/useDeleteFlow', () => ({
  useDeleteFlow: () => ({
    deleteDialogOpen: false,
    setDeleteDialogOpen: vi.fn(),
    deletingId: null,
    requestDelete: vi.fn(),
    confirmDelete: vi.fn(),
  }),
}))

vi.mock('@/store/hooks', () => ({
  // Async-thunk dispatches are unwrapped (`.unwrap()`) to surface failures, so
  // the mock returns an unwrappable handle rather than a bare value.
  useAppDispatch: () => vi.fn(() => ({ unwrap: () => Promise.resolve() })),
}))

describe('useHistoryController', () => {
  beforeEach(() => {
    clearHistorySessionSnapshot()
    historyDataState.baseItems = items
    vi.clearAllMocks()
  })

  it('persists the selected history entry as soon as selection changes', async () => {
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
        model: { query: '', timeRange: 'all_time' },
        items,
        total: items.length,
        hasMore: false,
        state: 'ready',
      },
      selectedId: 'entry-1',
      seenIds: ['entry-1'],
      scrollState: null,
    })

    const { result } = renderHook(() => useHistoryController())

    await act(async () => {
      result.current.handleCardClick('entry-2')
    })

    await waitFor(() => {
      expect(readHistorySessionSnapshot()?.selectedId).toBe('entry-2')
    })
  })

  it('registers a shortcut to favorite the selected preview entry', async () => {
    const { result } = renderHook(() => useHistoryController())

    await waitFor(() => {
      expect(result.current.selectedItem?.id).toBe('entry-1')
    })

    const shortcutConfigs = vi.mocked(useShortcut).mock.calls.map(([config]) => config)
    const favoriteShortcut = [...shortcutConfigs]
      .reverse()
      .find(config => config.id === 'clipboard.favorite' && config.scope === 'clipboard')

    expect(favoriteShortcut).toBeDefined()

    await act(async () => {
      await favoriteShortcut?.handler()
    })

    await waitFor(() => {
      expect(result.current.selectedItem?.isFavorited).toBe(true)
    })
    expect(clipboardItemsApi.favoriteClipboardItem).toHaveBeenCalledWith('entry-1')
  })

  it('keeps the active row on the first card when a newer card is prepended', async () => {
    const { result, rerender } = renderHook(() => useHistoryController())

    await waitFor(() => {
      expect(result.current.selectedId).toBe('entry-1')
    })

    const incoming: DisplayClipboardItem = {
      id: 'entry-new',
      type: 'text',
      content: { display_text: 'new', has_detail: false, size: 3 },
      activeTime: 3,
    }

    await act(async () => {
      historyDataState.baseItems = [incoming, ...items]
      rerender()
    })

    await waitFor(() => {
      expect(result.current.selectedId).toBe('entry-new')
    })
    expect(result.current.selectedItem?.id).toBe('entry-new')
  })

  it('keeps the selected id when existing cards are only reordered', async () => {
    const { result, rerender } = renderHook(() => useHistoryController())

    await waitFor(() => {
      expect(result.current.selectedId).toBe('entry-1')
    })

    await act(async () => {
      historyDataState.baseItems = [items[1], items[0]]
      rerender()
    })

    await waitFor(() => {
      expect(result.current.selectedId).toBe('entry-1')
    })
    expect(result.current.selectedItem?.id).toBe('entry-1')
  })
})
