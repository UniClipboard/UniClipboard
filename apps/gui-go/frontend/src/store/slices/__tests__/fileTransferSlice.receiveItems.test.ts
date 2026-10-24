import { describe, expect, it } from 'vitest'
import type { ReceiveSnapshot } from '@/lib/receive-items'
import fileTransferReducer, {
  applyReceiveSnapshot,
  markReceiveCancellationIncomplete,
  selectReceiveCancellationIncomplete,
  selectReceiveSnapshot,
  setReceiveAttemptState,
  updateTransferProgress,
} from '../fileTransferSlice'

const empty = {
  activeTransfers: {},
  entryTransferMap: {},
  entryStatusById: {},
}

function snap(attemptId: string, state: string, items: ReceiveSnapshot['items']): ReceiveSnapshot {
  return {
    entryId: 'entry',
    attemptId,
    state,
    totalBytes: 200,
    completedBytes: 100,
    itemsTotal: items.length,
    itemsCompleted: items.filter(item => item.state === 'completed').length,
    items,
  }
}

const item = (itemId: string, state: string) => ({
  itemId,
  kind: 'file',
  state,
  failureReason: null,
})

const root = (fileTransfer: ReturnType<typeof fileTransferReducer>) =>
  ({ fileTransfer }) as Parameters<typeof selectReceiveSnapshot>[0]

describe('receive snapshots', () => {
  it('stores the per-file result next to the aggregate attempt', () => {
    const state = fileTransferReducer(
      empty,
      applyReceiveSnapshot(snap('a1', 'failed', [item('x', 'completed'), item('y', 'failed')]))
    )
    const stored = selectReceiveSnapshot(root(state), 'entry')
    expect(stored?.items.map(i => [i.itemId, i.state])).toEqual([
      ['x', 'completed'],
      ['y', 'failed'],
    ])
    expect(state.currentAttemptByEntry?.entry).toBe('a1')
  })

  it('keeps the completed file visible when the retry attempt starts', () => {
    let state = fileTransferReducer(
      empty,
      applyReceiveSnapshot(snap('a1', 'failed', [item('x', 'completed'), item('y', 'failed')]))
    )
    state = fileTransferReducer(
      state,
      applyReceiveSnapshot(
        snap('a2', 'receiving', [item('x', 'completed'), item('y', 'transferring')])
      )
    )
    const stored = selectReceiveSnapshot(root(state), 'entry')
    expect(stored?.attemptId).toBe('a2')
    expect(stored?.items.find(i => i.itemId === 'x')?.state).toBe('completed')
    expect(stored?.items.find(i => i.itemId === 'y')?.state).toBe('transferring')
  })

  it('ignores a late snapshot of a superseded attempt', () => {
    let state = fileTransferReducer(
      empty,
      applyReceiveSnapshot(snap('a1', 'failed', [item('y', 'failed')]))
    )
    state = fileTransferReducer(
      state,
      applyReceiveSnapshot(snap('a2', 'receiving', [item('y', 'transferring')]))
    )
    state = fileTransferReducer(
      state,
      applyReceiveSnapshot(snap('a1', 'failed', [item('y', 'failed')]))
    )
    expect(selectReceiveSnapshot(root(state), 'entry')?.attemptId).toBe('a2')
    expect(state.currentAttemptByEntry?.entry).toBe('a2')
  })
})

describe('snapshots racing with events of the same attempt', () => {
  it('does not move a settled attempt back to running', () => {
    let state = fileTransferReducer(
      empty,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'failed' })
    )
    // A query that was answered before the failure but delivered after the event.
    state = fileTransferReducer(
      state,
      applyReceiveSnapshot(snap('a1', 'receiving', [item('y', 'transferring')]))
    )
    expect(state.entryStatusById.entry.status).toBe('failed')
    expect(selectReceiveSnapshot(root(state), 'entry')).toBeUndefined()
  })

  it('applies the settled snapshot over a running one', () => {
    let state = fileTransferReducer(
      empty,
      applyReceiveSnapshot(snap('a1', 'receiving', [item('y', 'transferring')]))
    )
    state = fileTransferReducer(
      state,
      applyReceiveSnapshot(snap('a1', 'failed', [item('y', 'failed')]))
    )
    expect(selectReceiveSnapshot(root(state), 'entry')?.state).toBe('failed')
    expect(state.entryStatusById.entry.status).toBe('failed')
  })
})

describe('late events of an old attempt', () => {
  it('cannot take the entry back from the new attempt', () => {
    let state = fileTransferReducer(
      empty,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'failed' })
    )
    state = fileTransferReducer(
      state,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a2', state: 'receiving' })
    )
    // A delayed `receiving` event of a1 must not revive it, whatever its state.
    state = fileTransferReducer(
      state,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'receiving' })
    )
    state = fileTransferReducer(
      state,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'failed' })
    )
    expect(state.currentAttemptByEntry?.entry).toBe('a2')
    expect(state.entryStatusById.entry.status).toBe('transferring')
  })

  it('cannot move progress of the new attempt', () => {
    let state = fileTransferReducer(
      empty,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'failed' })
    )
    state = fileTransferReducer(
      state,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a2', state: 'receiving' })
    )
    state = fileTransferReducer(
      state,
      updateTransferProgress({
        transferId: 'blob-1',
        entryId: 'entry',
        attemptId: 'a1',
        peerId: 'peer',
        direction: 'receiving',
        bytesTransferred: 999,
        totalBytes: 999,
      })
    )
    expect(state.activeTransfers[`entry\0a1`]).toBeUndefined()
    expect(state.currentAttemptByEntry?.entry).toBe('a2')
  })
})

describe('incomplete cancellation', () => {
  it('is remembered for the attempt and never recorded as cancelled', () => {
    let state = fileTransferReducer(
      empty,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'receiving' })
    )
    state = fileTransferReducer(
      state,
      markReceiveCancellationIncomplete({ entryId: 'entry', attemptId: 'a1' })
    )
    expect(selectReceiveCancellationIncomplete(root(state), 'entry')).toBe(true)
    expect(state.entryStatusById.entry.status).toBe('transferring')
  })

  it('clears once the same attempt actually settles', () => {
    let state = fileTransferReducer(
      empty,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'receiving' })
    )
    state = fileTransferReducer(
      state,
      markReceiveCancellationIncomplete({ entryId: 'entry', attemptId: 'a1' })
    )
    state = fileTransferReducer(
      state,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'cancelled' })
    )
    expect(selectReceiveCancellationIncomplete(root(state), 'entry')).toBe(false)
    expect(state.entryStatusById.entry.status).toBe('cancelled')
  })

  it('does not outlive the attempt it was recorded for', () => {
    let state = fileTransferReducer(
      empty,
      setReceiveAttemptState({ entryId: 'entry', attemptId: 'a1', state: 'receiving' })
    )
    state = fileTransferReducer(
      state,
      markReceiveCancellationIncomplete({ entryId: 'entry', attemptId: 'a1' })
    )
    state = fileTransferReducer(
      state,
      applyReceiveSnapshot(snap('a2', 'receiving', [item('y', 'transferring')]))
    )
    expect(selectReceiveCancellationIncomplete(root(state), 'entry')).toBe(false)
  })
})
