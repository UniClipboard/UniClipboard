import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import {
  clearHistorySessionSnapshot,
  readHistorySessionSnapshot,
  writeHistorySessionSnapshot,
} from '../historySessionSnapshot'
import { useHistoryController } from '../useHistoryController'

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
    baseItems: items,
    liveSnapshot: {
      model: { query: '', timeRange: 'all_time' },
      items,
      total: items.length,
      hasMore: false,
      state: 'ready',
    },
    browseCount: items.length,
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

vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: vi.fn(),
}))

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
  useAppDispatch: () => vi.fn(),
}))

describe('useHistoryController', () => {
  beforeEach(() => {
    clearHistorySessionSnapshot()
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
})
