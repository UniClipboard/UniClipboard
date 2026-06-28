import { configureStore } from '@reduxjs/toolkit'
import { renderHook, waitFor } from '@testing-library/react'
import React from 'react'
import { Provider } from 'react-redux'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import { querySearch } from '@/api/daemon/search'
import clipboardReducer from '@/store/slices/clipboardSlice'
import devicesReducer from '@/store/slices/devicesSlice'
import {
  clearHistorySessionSnapshot,
  readHistorySessionSnapshot,
  writeHistorySessionSnapshot,
} from '../historySessionSnapshot'
import { useHistoryData } from '../useHistoryData'

vi.mock('@/api/daemon/search', async () => {
  const actual = await vi.importActual<typeof import('@/api/daemon/search')>('@/api/daemon/search')
  return {
    ...actual,
    querySearch: vi.fn(),
    getSearchTags: vi.fn(),
  }
})

vi.mock('@/api/daemon', () => ({
  getEncryptionState: vi.fn().mockResolvedValue({ initialized: false, sessionReady: false }),
}))

vi.mock('@/api/tauri-command/mobile_sync', () => ({
  listMobileDevices: vi.fn().mockResolvedValue([]),
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn(() => () => {}),
    onReconnect: vi.fn(() => () => {}),
  },
}))

function createWrapper() {
  const store = configureStore({
    reducer: {
      clipboard: clipboardReducer,
      devices: devicesReducer,
    },
  })
  const Wrapper = ({ children }: { children: React.ReactNode }) => (
    <Provider store={store}>{children}</Provider>
  )
  return Wrapper
}

describe('useHistoryData', () => {
  beforeEach(() => {
    clearHistorySessionSnapshot()
    vi.clearAllMocks()
  })

  it('shows the session snapshot immediately while refreshing in the background', async () => {
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
        items: [
          {
            id: 'cached-entry',
            type: 'text',
            content: { display_text: 'cached', has_detail: false, size: 6 },
            activeTime: 1,
          },
        ],
        total: 1,
        hasMore: false,
        state: 'ready',
      },
      selectedId: 'cached-entry',
      seenIds: ['cached-entry'],
      scrollState: null,
    })

    vi.mocked(querySearch).mockResolvedValue({
      data: {
        items: [
          {
            entryId: 'fresh-entry',
            contentType: 'text',
            activeTimeMs: 2,
            tags: [],
            textPreview: 'fresh',
            charCount: 5,
            mimeType: 'text/plain',
            fileExtensions: [],
            fileNames: [],
            linkUrls: [],
            sourceDevice: null,
            payloadState: null,
          },
        ],
        total: 1,
        hasMore: false,
        state: 'ready',
      },
      ts: 2,
    })

    const { result } = renderHook(() => useHistoryData(), { wrapper: createWrapper() })

    expect(result.current.baseItems.map(item => item.id)).toEqual(['cached-entry'])
    expect(querySearch).toHaveBeenCalled()

    await waitFor(() => {
      expect(result.current.baseItems.map(item => item.id)).toEqual(['fresh-entry'])
    })
  })

  it('writes a reusable snapshot after the first successful history load', async () => {
    vi.mocked(querySearch).mockResolvedValue({
      data: {
        items: [
          {
            entryId: 'fresh-entry',
            contentType: 'text',
            activeTimeMs: 2,
            tags: [],
            textPreview: 'fresh',
            charCount: 5,
            mimeType: 'text/plain',
            fileExtensions: [],
            fileNames: [],
            linkUrls: [],
            sourceDevice: null,
            payloadState: null,
          },
        ],
        total: 1,
        hasMore: false,
        state: 'ready',
      },
      ts: 2,
    })

    const { result } = renderHook(() => useHistoryData(), { wrapper: createWrapper() })

    await waitFor(() => {
      expect(result.current.baseItems.map(item => item.id)).toEqual(['fresh-entry'])
    })

    expect(readHistorySessionSnapshot()?.live?.items.map(item => item.id)).toEqual(['fresh-entry'])
  })
})
