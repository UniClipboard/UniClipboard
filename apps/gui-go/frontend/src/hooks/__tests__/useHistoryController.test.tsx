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
  offset: 0,
  key: 'browse',
  atTop: true,
  total: 200,
  ensureItem: vi.fn(),
  ensureNeighbor: vi.fn(),
  moveToTop: vi.fn(),
  refetch: vi.fn(),
  remove: vi.fn(),
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
    viewport: {
      offset: historyDataState.offset,
      total: historyDataState.total,
      onVisibleRange: vi.fn(),
      onNavigate: vi.fn(),
    },
    resultKey: historyDataState.key,
    isAtTop: () => historyDataState.atTop,
    resultsReady: true,
    ensureItem: historyDataState.ensureItem,
    ensureNeighbor: historyDataState.ensureNeighbor,
    moveToTop: historyDataState.moveToTop,
    refetch: historyDataState.refetch,
    baseItems: historyDataState.baseItems,
    liveSnapshot: {
      model: { query: '', timeRange: 'all_time' },
      items: historyDataState.baseItems,
      total: historyDataState.baseItems.length,
      hasMore: false,
      state: 'ready',
    },
    browseCount: historyDataState.total,
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
    markCopied: vi.fn(),
  }),
}))

vi.mock('@/hooks/useDeleteFlow', () => ({
  useDeleteFlow: (remove: (id: string) => Promise<void>) => {
    historyDataState.remove.mockImplementation(remove)
    return {
      deleteDialogOpen: false,
      setDeleteDialogOpen: vi.fn(),
      deletingId: null,
      requestDelete: vi.fn(),
      confirmDelete: vi.fn(),
    }
  },
}))

vi.mock('@/store/hooks', () => ({
  // Async-thunk dispatches are unwrapped (`.unwrap()`) to surface failures, so
  // the mock returns an unwrappable handle rather than a bare value.
  useAppDispatch: () => vi.fn(() => ({ unwrap: () => Promise.resolve() })),
}))

describe('useHistoryController', () => {
  beforeEach(() => {
    clearHistorySessionSnapshot()
    historyDataState.offset = 0
    historyDataState.key = 'browse'
    historyDataState.atTop = true
    historyDataState.total = 200
    historyDataState.baseItems = items
    vi.clearAllMocks()
  })

  it('updates the hovered shortcut target without rendering the page', () => {
    let renders = 0
    const { result } = renderHook(() => {
      renders += 1
      return useHistoryController()
    })
    const initialRenders = renders
    act(() => result.current.handleHoverChange('entry-1', true))
    act(() => result.current.handleHoverChange('entry-2', true))
    act(() => result.current.handleHoverChange('entry-2', false))
    expect(renders).toBe(initialRenders)
  })

  it('keeps hover shortcuts current and ignores releases from other rows', () => {
    const { result, rerender } = renderHook(() => useHistoryController())
    const deleteShortcut = () =>
      vi
        .mocked(useShortcut)
        .mock.calls.map(([config]) => config)
        .reverse()
        .find(config => config.key === 'd')!
    const requestDelete = result.current.requestDelete

    act(() => result.current.handleHoverChange('entry-1', true))
    act(() => result.current.handleHoverChange('entry-2', true))
    act(() => result.current.handleHoverChange('entry-1', false))
    act(() => deleteShortcut().handler())
    expect(requestDelete).toHaveBeenCalledWith('entry-2')

    vi.mocked(requestDelete).mockClear()
    act(() => result.current.handleHoverChange('entry-2', false))
    act(() => deleteShortcut().handler())
    expect(requestDelete).not.toHaveBeenCalled()

    act(() => result.current.handleHoverChange('entry-2', true))
    historyDataState.baseItems = [items[0]]
    rerender()
    act(() => deleteShortcut().handler())
    expect(result.current.requestDelete).not.toHaveBeenCalled()
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
  it('keeps the selected projection and bulk ids when scrolling evicts their rows, updating on reload', () => {
    const { result, rerender } = renderHook(() => useHistoryController())
    act(() => {
      result.current.handleCardClick('entry-2')
      result.current.setChecked(['entry-1', 'entry-2'])
    })
    historyDataState.offset = 80
    historyDataState.baseItems = [{ ...items[0], id: 'deep' }]
    rerender()
    expect(result.current.selectedId).toBe('entry-2')
    expect(result.current.selectedItem).toEqual(items[1])
    expect([...result.current.checkedIds]).toEqual(['entry-1', 'entry-2'])
    historyDataState.offset = 0
    historyDataState.baseItems = [{ ...items[1], textPreview: 'updated' }]
    rerender()
    expect(result.current.selectedItem?.textPreview).toBe('updated')
  })

  it('navigates arrows relative to the evicted selected absolute index', async () => {
    const { result, rerender } = renderHook(() => useHistoryController())
    act(() => result.current.handleCardClick('entry-2'))
    historyDataState.offset = 80
    historyDataState.baseItems = [{ ...items[0], id: 'deep' }]
    rerender()
    const neighbor = { ...items[0], id: 'neighbor' }
    historyDataState.ensureNeighbor
      .mockResolvedValueOnce({ item: neighbor, index: 2 })
      .mockResolvedValueOnce({ item: items[1], index: 1 })
    await act(async () => {
      await result.current.viewport.onMoveSelection(1)
    })
    expect(historyDataState.ensureNeighbor).toHaveBeenCalledWith('entry-2', 1, 1)
    expect(result.current.selectedItem?.id).toBe('neighbor')
    await act(async () => {
      await result.current.viewport.onMoveSelection(-1)
    })
    expect(historyDataState.ensureNeighbor).toHaveBeenLastCalledWith('neighbor', 2, -1)
  })

  it('does not follow the first row of a deep window when new entries arrive', () => {
    historyDataState.offset = 80
    const { result, rerender } = renderHook(() => useHistoryController())
    historyDataState.baseItems = [{ ...items[0], id: 'new-deep' }, ...items]
    rerender()
    expect(result.current.selectedId).toBe('entry-1')
  })

  it('clears out-of-results selection and bulk ids when the query changes', () => {
    const { result, rerender } = renderHook(() => useHistoryController())
    act(() => result.current.setChecked(['entry-1', 'entry-2']))
    historyDataState.key = 'filtered'
    historyDataState.baseItems = []
    rerender()
    expect(result.current.selectedId).toBeNull()
    expect(result.current.selectedItem).toBeNull()
    expect(result.current.checkedIds.size).toBe(0)
    historyDataState.baseItems = [items[1]]
    rerender()
    expect(result.current.selectedId).toBe('entry-2')
  })

  it('clears an evicted selected entry after confirmed deletion', async () => {
    const { result, rerender } = renderHook(() => useHistoryController())
    act(() => result.current.setChecked(['entry-1', 'entry-2']))
    historyDataState.offset = 80
    historyDataState.baseItems = [{ ...items[0], id: 'deep' }]
    rerender()
    await act(async () => {
      await historyDataState.remove('entry-1')
    })
    expect(result.current.selectedId).not.toBe('entry-1')
    expect(result.current.selectedItem?.id).not.toBe('entry-1')
    expect(result.current.checkedIds.has('entry-1')).toBe(false)
  })

  it('preserves the selection when it belongs to the first window of a new query', () => {
    const { result, rerender } = renderHook(() => useHistoryController())
    act(() => result.current.handleCardClick('entry-2'))
    historyDataState.key = 'filtered'
    historyDataState.baseItems = [items[1]]
    rerender()
    expect(result.current.selectedId).toBe('entry-2')
  })

  it('does not follow arrivals when the visible range is below the top even at offset zero', () => {
    const { result, rerender } = renderHook(() => useHistoryController())
    historyDataState.atTop = false
    historyDataState.baseItems = [{ ...items[0], id: 'incoming' }, ...items]
    rerender()
    expect(result.current.selectedId).toBe('entry-1')
  })

  it('pins checked identities even after their records leave the window', async () => {
    const { result, rerender } = renderHook(() => useHistoryController())
    act(() => result.current.setChecked(['entry-1', 'entry-2']))
    historyDataState.offset = 80
    historyDataState.baseItems = [{ ...items[0], id: 'deep' }]
    rerender()
    await act(async () => result.current.pinChecked())
    expect(clipboardItemsApi.favoriteClipboardItem).toHaveBeenCalledWith('entry-1')
    expect(clipboardItemsApi.favoriteClipboardItem).toHaveBeenCalledWith('entry-2')
  })
  it('keeps the selected detail through copy promotion without scrolling and updates its absolute index at the head', async () => {
    historyDataState.offset = 80
    const { result, rerender } = renderHook(() => useHistoryController())
    act(() => result.current.handleCardClick('entry-2'))
    historyDataState.offset = 120
    historyDataState.baseItems = [{ ...items[0], id: 'deep' }]
    rerender()
    await act(async () => {
      await result.current.handleCopy('entry-2')
    })
    // Copy re-reads the current window in place; it neither scrolls to the head nor drops the detail.
    expect(historyDataState.refetch).toHaveBeenCalled()
    expect(historyDataState.moveToTop).not.toHaveBeenCalled()
    historyDataState.baseItems = [{ ...items[0], id: 'deep-refreshed' }]
    rerender()
    expect(result.current.items[0].id).toBe('deep-refreshed')
    expect(result.current.selectedItem?.id).toBe('entry-2')
    historyDataState.offset = 0
    historyDataState.baseItems = [items[1], items[0]]
    rerender()
    expect(result.current.selectedId).toBe('entry-2')
    historyDataState.ensureItem.mockResolvedValue(items[0])
    await act(async () => {
      await result.current.viewport.onMoveSelection(1)
    })
    expect(historyDataState.ensureItem).toHaveBeenLastCalledWith(1)
  })
})
