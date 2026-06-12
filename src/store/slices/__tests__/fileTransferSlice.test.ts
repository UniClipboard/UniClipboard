import { describe, it, expect } from 'vitest'
import fileTransferReducer, {
  markTransferCancelled,
  normalizeCancelReason,
  resolveEntryTransferStatus,
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

  describe('resolveEntryTransferStatus', () => {
    it('prefers live active progress over durable pending state', () => {
      const entryStatus: EntryTransferStatus = {
        status: 'pending',
        reason: null,
      }

      const transfer = {
        transferId: 'tx-live',
        entryId: 'entry-live',
        peerId: 'peer-1',
        direction: 'Sending' as const,
        bytesTransferred: 2048,
        totalBytes: 10240,
        status: 'active' as const,
        startedAt: 1000,
        updatedAt: 2000,
        bytesPerSecond: 2048,
        estimatedRemainingSeconds: 4,
      }

      expect(resolveEntryTransferStatus(entryStatus, transfer)).toBe('transferring')
    })

    it('keeps durable failed state when no live progress exists', () => {
      const entryStatus: EntryTransferStatus = {
        status: 'failed',
        reason: 'timeout',
      }

      expect(resolveEntryTransferStatus(entryStatus, undefined)).toBe('failed')
    })

    it('returns cancelled when durable status is cancelled', () => {
      const entryStatus: EntryTransferStatus = {
        status: 'cancelled',
        reason: 'local_user',
      }

      expect(resolveEntryTransferStatus(entryStatus, undefined)).toBe('cancelled')
    })

    it('returns cancelled when live transfer is cancelled', () => {
      const entryStatus: EntryTransferStatus = {
        status: 'transferring',
        reason: null,
      }

      const transfer = {
        transferId: 'tx-cancel',
        entryId: 'entry-cancel',
        peerId: 'peer-1',
        direction: 'Receiving' as const,
        bytesTransferred: 4096,
        totalBytes: 10240,
        status: 'cancelled' as const,
        cancelReason: 'local_user',
        startedAt: 1000,
        updatedAt: 2000,
        bytesPerSecond: null,
        estimatedRemainingSeconds: null,
      }

      expect(resolveEntryTransferStatus(entryStatus, transfer)).toBe('cancelled')
    })

    it('fallback: legacy "failed + cancelled:* reason" row renders as cancelled', () => {
      // 历史数据兼容:0.7.x 之前后端把 cancel 落成 status=failed + reason="cancelled:local_user",
      // resolver 应当识别旧 prefix 并按 cancelled 渲染,与新数据视觉一致。
      const entryStatus: EntryTransferStatus = {
        status: 'failed',
        reason: 'cancelled:remote_peer',
      }

      expect(resolveEntryTransferStatus(entryStatus, undefined)).toBe('cancelled')
    })

    it('genuine failed state with non-cancel reason stays failed', () => {
      const entryStatus: EntryTransferStatus = {
        status: 'failed',
        reason: 'network_unavailable',
      }

      expect(resolveEntryTransferStatus(entryStatus, undefined)).toBe('failed')
    })
  })

  describe('normalizeCancelReason', () => {
    it('strips legacy "cancelled:" prefix', () => {
      expect(normalizeCancelReason('cancelled:local_user')).toBe('local_user')
      expect(normalizeCancelReason('cancelled:timeout')).toBe('timeout')
    })

    it('passes through bare label unchanged', () => {
      expect(normalizeCancelReason('local_user')).toBe('local_user')
    })

    it('returns undefined for empty / null input', () => {
      expect(normalizeCancelReason(null)).toBeUndefined()
      expect(normalizeCancelReason(undefined)).toBeUndefined()
      expect(normalizeCancelReason('')).toBeUndefined()
    })
  })

  describe('markTransferCancelled', () => {
    it('flips active transfer status to cancelled and records reason', () => {
      const state = {
        activeTransfers: {
          'tx-1': {
            transferId: 'tx-1',
            entryId: 'entry-1',
            peerId: 'peer-1',
            direction: 'Receiving' as const,
            bytesTransferred: 1024,
            totalBytes: 4096,
            status: 'active' as const,
            startedAt: 1000,
            updatedAt: 2000,
            bytesPerSecond: 1024,
            estimatedRemainingSeconds: 3,
          },
        },
        entryTransferMap: { 'entry-1': 'tx-1' },
        entryStatusById: {},
      }

      const next = fileTransferReducer(
        state,
        markTransferCancelled({ transferId: 'tx-1', reason: 'local_user' })
      )

      const tx = next.activeTransfers['tx-1']
      expect(tx.status).toBe('cancelled')
      expect(tx.cancelReason).toBe('local_user')
      expect(tx.estimatedRemainingSeconds).toBeNull()
    })

    it('is a no-op when transferId not present', () => {
      const next = fileTransferReducer(
        initialState,
        markTransferCancelled({ transferId: 'missing', reason: 'unknown' })
      )
      expect(next.activeTransfers).toEqual({})
    })
  })
})
