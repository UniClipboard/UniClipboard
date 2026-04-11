import { describe, it, expect } from 'vitest'
import fileTransferReducer, {
  setEntryTransferStatus,
  hydrateEntryTransferStatuses,
  removeEntryTransferStatus,
  updateTransferProgress,
  removeTransfer,
} from '../fileTransferSlice'
import type { EntryTransferStatus } from '../fileTransferSlice'

const initialState = {
  activeTransfers: {},
  entryTransferMap: {},
  entryStatusById: {},
}

describe('fileTransferSlice - file transfer status', () => {
  describe('setEntryTransferStatus', () => {
    it('sets durable entry status from live status event to transferring', () => {
      const state = fileTransferReducer(
        initialState,
        setEntryTransferStatus({ entryId: 'entry-1', status: 'transferring' })
      )

      expect(state.entryStatusById['entry-1']).toEqual({
        status: 'transferring',
        reason: null,
      })
    })

    it('sets failed status with reason', () => {
      const state = fileTransferReducer(
        initialState,
        setEntryTransferStatus({
          entryId: 'entry-2',
          status: 'failed',
          reason: 'timeout after 60s',
        })
      )

      expect(state.entryStatusById['entry-2']).toEqual({
        status: 'failed',
        reason: 'timeout after 60s',
      })
    })

    it('overwrites existing durable status on subsequent event', () => {
      const withPending = fileTransferReducer(
        initialState,
        setEntryTransferStatus({ entryId: 'entry-1', status: 'pending' })
      )
      const withTransferring = fileTransferReducer(
        withPending,
        setEntryTransferStatus({ entryId: 'entry-1', status: 'transferring' })
      )

      expect(withTransferring.entryStatusById['entry-1'].status).toBe('transferring')
    })
  })

  describe('hydrateEntryTransferStatuses', () => {
    it('bulk-hydrates durable statuses from initial API query', () => {
      const entries: Array<{
        entryId: string
        status: EntryTransferStatus['status']
        reason?: string | null
      }> = [
        { entryId: 'e1', status: 'failed', reason: 'hash mismatch' },
        { entryId: 'e2', status: 'pending' },
        { entryId: 'e3', status: 'completed' },
      ]

      const state = fileTransferReducer(initialState, hydrateEntryTransferStatuses(entries))

      expect(state.entryStatusById['e1']).toEqual({
        status: 'failed',
        reason: 'hash mismatch',
      })
      expect(state.entryStatusById['e2']).toEqual({ status: 'pending', reason: null })
      expect(state.entryStatusById['e3']).toEqual({ status: 'completed', reason: null })
    })
  })

  describe('removeEntryTransferStatus', () => {
    it('removes durable status for deleted entry', () => {
      const withStatus = fileTransferReducer(
        initialState,
        setEntryTransferStatus({ entryId: 'entry-1', status: 'completed' })
      )
      const afterRemove = fileTransferReducer(withStatus, removeEntryTransferStatus('entry-1'))

      expect(afterRemove.entryStatusById['entry-1']).toBeUndefined()
    })
  })

  describe('progress cleanup does not erase durable entry status', () => {
    it('removeTransfer clears progress but leaves entryStatusById intact', () => {
      // Set up both progress and durable status
      let state = fileTransferReducer(
        initialState,
        updateTransferProgress({
          transferId: 'tx-1',
          peerId: 'peer-1',
          direction: 'Receiving',
          chunksCompleted: 5,
          totalChunks: 5,
          bytesTransferred: 1000,
          totalBytes: 1000,
        })
      )
      state = fileTransferReducer(
        state,
        setEntryTransferStatus({ entryId: 'entry-1', status: 'completed' })
      )

      // Simulate auto-clear of completed transfer progress
      state = fileTransferReducer(state, removeTransfer('tx-1'))

      // Progress state is gone
      expect(state.activeTransfers['tx-1']).toBeUndefined()
      // Durable status persists
      expect(state.entryStatusById['entry-1']).toEqual({
        status: 'completed',
        reason: null,
      })
    })
  })

  describe('updateTransferProgress', () => {
    it('computes speed and remaining time from live progress updates', () => {
      let state = fileTransferReducer(
        initialState,
        updateTransferProgress({
          transferId: 'tx-speed',
          entryId: 'entry-speed',
          peerId: 'peer-1',
          direction: 'Receiving',
          chunksCompleted: 1,
          totalChunks: 4,
          bytesTransferred: 1024,
          totalBytes: 4096,
          eventTs: 1000,
        })
      )

      state = fileTransferReducer(
        state,
        updateTransferProgress({
          transferId: 'tx-speed',
          entryId: 'entry-speed',
          peerId: 'peer-1',
          direction: 'Receiving',
          chunksCompleted: 2,
          totalChunks: 4,
          bytesTransferred: 2048,
          totalBytes: 4096,
          eventTs: 2000,
        })
      )

      expect(state.entryTransferMap['entry-speed']).toBe('tx-speed')
      expect(state.activeTransfers['tx-speed'].bytesPerSecond).toBe(2048)
      expect(state.activeTransfers['tx-speed'].estimatedRemainingSeconds).toBe(1)
    })
  })
})
