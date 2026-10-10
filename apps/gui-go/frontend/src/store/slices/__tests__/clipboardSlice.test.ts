import { describe, it, expect } from 'vitest'
import clipboardReducer, {
  addPendingEntry,
  removePendingEntry,
  type PendingClipboardEntry,
} from '../clipboardSlice'

function makePending(
  entryId: string,
  overrides?: Partial<PendingClipboardEntry>
): PendingClipboardEntry {
  return {
    entryId,
    fromDevice: 'peer-1',
    totalBytes: null,
    filenames: [],
    createdAt: 0,
    ...overrides,
  }
}

describe('clipboardSlice reducers', () => {
  describe('addPendingEntry', () => {
    it('inserts a pending placeholder at index 0', () => {
      const result = clipboardReducer({ pendingItems: [] }, addPendingEntry(makePending('a')))

      expect(result.pendingItems).toHaveLength(1)
      expect(result.pendingItems[0].entryId).toBe('a')
    })

    it('unshifts newer placeholders ahead of older ones', () => {
      const state = { pendingItems: [makePending('a')] }

      const result = clipboardReducer(state, addPendingEntry(makePending('b')))

      expect(result.pendingItems.map(p => p.entryId)).toEqual(['b', 'a'])
    })

    it('replaces an existing placeholder with the same entryId (idempotent)', () => {
      const state = { pendingItems: [makePending('a', { totalBytes: 1 })] }

      const result = clipboardReducer(state, addPendingEntry(makePending('a', { totalBytes: 99 })))

      expect(result.pendingItems).toHaveLength(1)
      expect(result.pendingItems[0].totalBytes).toBe(99)
    })
  })

  describe('removePendingEntry', () => {
    it('drops the placeholder for the given entryId', () => {
      const state = { pendingItems: [makePending('a'), makePending('b')] }

      const result = clipboardReducer(state, removePendingEntry('a'))

      expect(result.pendingItems.map(p => p.entryId)).toEqual(['b'])
    })

    it('leaves state unchanged for a non-existent entryId', () => {
      const state = { pendingItems: [makePending('a')] }

      const result = clipboardReducer(state, removePendingEntry('z'))

      expect(result.pendingItems).toHaveLength(1)
    })
  })

  describe('initial state', () => {
    it('has only the pending overlay', () => {
      const state = clipboardReducer(undefined, { type: 'unknown' })

      expect(state).toEqual({ pendingItems: [] })
    })
  })
})
