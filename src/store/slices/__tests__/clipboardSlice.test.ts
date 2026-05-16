import { configureStore } from '@reduxjs/toolkit'
import { describe, it, expect } from 'vitest'
import type { ClipboardItemResponse, ClipboardItemsResult } from '@/api/clipboardItems'
import clipboardReducer, { prependItem, removeItem } from '../clipboardSlice'
import fileTransferReducer from '../fileTransferSlice'

function makeItem(id: string, overrides?: Partial<ClipboardItemResponse>): ClipboardItemResponse {
  return {
    id,
    is_downloaded: true,
    is_favorited: false,
    created_at: Date.now(),
    updated_at: Date.now(),
    active_time: 0,
    item: {
      text: { display_text: `text-${id}`, has_detail: false, size: 10 },
      image: null,
      file: null,
      link: null,
      code: null,
      unknown: null,
    },
    ...overrides,
  }
}

function makeStore() {
  return configureStore({
    reducer: {
      clipboard: clipboardReducer,
      fileTransfer: fileTransferReducer,
    },
  })
}

const initialState = {
  items: [],
  pendingItems: [],
  loading: false,
  notReady: false,
  error: null,
  deleteConfirmId: null,
  staleEntryIds: [],
}

describe('clipboardSlice reducers', () => {
  describe('prependItem', () => {
    it('inserts item at index 0', () => {
      const existing = makeItem('a')
      const state = { ...initialState, items: [existing] }
      const newItem = makeItem('b')

      const result = clipboardReducer(state, prependItem(newItem))

      expect(result.items).toHaveLength(2)
      expect(result.items[0].id).toBe('b')
      expect(result.items[1].id).toBe('a')
    })

    it('does not insert duplicate entry_id', () => {
      const existing = makeItem('a')
      const state = { ...initialState, items: [existing] }
      const duplicate = makeItem('a')

      const result = clipboardReducer(state, prependItem(duplicate))

      expect(result.items).toHaveLength(1)
    })

    it('inserts into empty items array', () => {
      const newItem = makeItem('first')

      const result = clipboardReducer(initialState, prependItem(newItem))

      expect(result.items).toHaveLength(1)
      expect(result.items[0].id).toBe('first')
    })
  })

  describe('removeItem', () => {
    it('removes item by entry_id', () => {
      const items = [makeItem('a'), makeItem('b'), makeItem('c')]
      const state = { ...initialState, items }

      const result = clipboardReducer(state, removeItem('b'))

      expect(result.items).toHaveLength(2)
      expect(result.items.map(i => i.id)).toEqual(['a', 'c'])
    })

    it('leaves state unchanged for non-existent entry_id', () => {
      const items = [makeItem('a'), makeItem('b')]
      const state = { ...initialState, items }

      const result = clipboardReducer(state, removeItem('z'))

      expect(result.items).toHaveLength(2)
    })
  })

  describe('initial state', () => {
    it('has correct shape', () => {
      const state = clipboardReducer(undefined, { type: 'unknown' })

      expect(state.items).toEqual([])
      expect(state.loading).toBe(false)
      expect(state.notReady).toBe(false)
      expect(state.error).toBeNull()
      expect(state.deleteConfirmId).toBeNull()
    })
  })
})

describe('fetchClipboardItems hydration', () => {
  it('dispatches hydrateEntryTransferStatuses for items with file_transfer_status', async () => {
    const itemWithStatus = makeItem('file-entry-1', {
      file_transfer_status: 'failed',
      file_transfer_reason: 'timeout',
    })
    const itemWithoutStatus = makeItem('text-entry-1', {
      file_transfer_status: null,
    })

    const result: ClipboardItemsResult = {
      status: 'ready',
      items: [itemWithStatus, itemWithoutStatus],
    }

    const store = makeStore()
    const { hydrateEntryTransferStatuses } = await import('../fileTransferSlice')

    // Simulate the hydration logic inside fetchClipboardItems thunk:
    // filter items with file_transfer_status, collect payloads, and dispatch.
    const statusEntries =
      result.status === 'ready'
        ? result.items
            .filter(item => item.file_transfer_status != null)
            .map(item => ({
              entryId: item.id,
              status: item.file_transfer_status as
                | 'pending'
                | 'transferring'
                | 'completed'
                | 'failed',
              reason: item.file_transfer_reason ?? null,
            }))
        : []

    store.dispatch(hydrateEntryTransferStatuses(statusEntries))

    const state = store.getState()
    expect(state.fileTransfer.entryStatusById['file-entry-1']).toEqual({
      status: 'failed',
      reason: 'timeout',
    })
    // Item without file_transfer_status should NOT appear in entryStatusById
    expect(state.fileTransfer.entryStatusById['text-entry-1']).toBeUndefined()
  })

  it('does not add items without file_transfer_status to entryStatusById', async () => {
    const { hydrateEntryTransferStatuses } = await import('../fileTransferSlice')
    const store = makeStore()

    const items: ClipboardItemResponse[] = [
      makeItem('a', { file_transfer_status: null }),
      makeItem('b', { file_transfer_status: undefined }),
    ]

    const statusEntries = items
      .filter(item => item.file_transfer_status != null)
      .map(item => ({
        entryId: item.id,
        status: item.file_transfer_status as 'pending' | 'transferring' | 'completed' | 'failed',
        reason: item.file_transfer_reason ?? null,
      }))

    // No entries should match the filter (both have null/undefined status)
    expect(statusEntries).toHaveLength(0)

    store.dispatch(hydrateEntryTransferStatuses(statusEntries))

    const state = store.getState()
    expect(Object.keys(state.fileTransfer.entryStatusById)).toHaveLength(0)
  })
})
