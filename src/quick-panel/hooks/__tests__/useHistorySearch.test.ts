import { renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import * as historySearchModule from '../useHistorySearch'
import { useHistorySearch } from '../useHistorySearch'

const liveItems = vi.hoisted((): { value: DisplayClipboardItem[] } => ({
  value: [],
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('@/hooks/useEncryptionSessionState', () => ({
  useEncryptionSessionState: () => ({ isLocked: false }),
}))

vi.mock('@/hooks/useLiveSearch', () => ({
  useLiveSearch: vi.fn(() => ({
    items: liveItems.value,
    isLoading: false,
    total: liveItems.value.length,
    hasMore: false,
    state: 'ready',
    growWindow: vi.fn(),
    refetch: vi.fn(),
    removeItem: vi.fn(),
    patchItem: vi.fn(),
  })),
}))

describe('useHistorySearch', () => {
  it('keeps full display items available for the quick preview pane', () => {
    liveItems.value = [
      {
        id: 'code-1',
        type: 'text',
        content: { display_text: 'const answer = 42', has_detail: false, size: 17 },
        contentTags: ['code'],
        activeTime: 1710000000000,
        isUnavailable: false,
      },
    ]

    const { result } = renderHook(() =>
      useHistorySearch({
        searchQuery: '',
        activeFilter: Filter.All,
        tagFilter: null,
        sourceFilter: null,
        extensionFilter: null,
        timeRange: 'all_time',
      })
    )

    expect(result.current.filteredItems[0]).toMatchObject({
      id: 'code-1',
      preview: 'const answer = 42',
    })
    expect(result.current.previewItems[0]).toMatchObject({
      id: 'code-1',
      type: 'text',
      content: { display_text: 'const answer = 42', has_detail: false, size: 17 },
      contentTags: ['code'],
    })
  })
})

describe('parseTokens', () => {
  it('is no longer the quick panel search contract', () => {
    expect('parseTokens' in historySearchModule).toBe(false)
  })
})
