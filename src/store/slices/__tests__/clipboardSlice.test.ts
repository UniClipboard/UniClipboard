import { configureStore } from '@reduxjs/toolkit'
import { describe, it, expect } from 'vitest'
import type { ClipboardEntry } from '@/lib/clipboard-entry'
import clipboardReducer, { prependItem, removeItem } from '../clipboardSlice'
import fileTransferReducer from '../fileTransferSlice'

function makeItem(id: string, overrides?: Partial<ClipboardEntry>): ClipboardEntry {
  return {
    id,
    type: 'text',
    content: { display_text: `text-${id}`, has_detail: false, size: 10 },
    createdAt: Date.now(),
    updatedAt: Date.now(),
    activeTime: 0,
    isFavorited: false,
    isUnavailable: false,
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
  // The thunk reads fileTransferStatus straight off the daemon DTOs and seeds
  // fileTransferSlice (the single owner of transfer status) — ClipboardEntry
  // deliberately carries none of it. These tests mirror that filter/map logic.
  type DtoStatusFields = {
    id: string
    fileTransferStatus: string | null | undefined
    fileTransferReason?: string | null
  }

  function buildStatusEntries(dtos: DtoStatusFields[]) {
    return dtos
      .filter(dto => dto.fileTransferStatus != null)
      .map(dto => ({
        entryId: dto.id,
        status: dto.fileTransferStatus as 'pending' | 'transferring' | 'completed' | 'failed',
        reason: dto.fileTransferReason ?? null,
      }))
  }

  it('dispatches hydrateEntryTransferStatuses for DTOs with fileTransferStatus', async () => {
    const store = makeStore()
    const { hydrateEntryTransferStatuses } = await import('../fileTransferSlice')

    const statusEntries = buildStatusEntries([
      { id: 'file-entry-1', fileTransferStatus: 'failed', fileTransferReason: 'timeout' },
      { id: 'text-entry-1', fileTransferStatus: null },
    ])

    store.dispatch(hydrateEntryTransferStatuses(statusEntries))

    const state = store.getState()
    expect(state.fileTransfer.entryStatusById['file-entry-1']).toEqual({
      status: 'failed',
      reason: 'timeout',
    })
    // DTO without fileTransferStatus should NOT appear in entryStatusById
    expect(state.fileTransfer.entryStatusById['text-entry-1']).toBeUndefined()
  })

  it('does not add DTOs without fileTransferStatus to entryStatusById', async () => {
    const { hydrateEntryTransferStatuses } = await import('../fileTransferSlice')
    const store = makeStore()

    const statusEntries = buildStatusEntries([
      { id: 'a', fileTransferStatus: null },
      { id: 'b', fileTransferStatus: undefined },
    ])

    // No entries should match the filter (both have null/undefined status)
    expect(statusEntries).toHaveLength(0)

    store.dispatch(hydrateEntryTransferStatuses(statusEntries))

    const state = store.getState()
    expect(Object.keys(state.fileTransfer.entryStatusById)).toHaveLength(0)
  })
})
