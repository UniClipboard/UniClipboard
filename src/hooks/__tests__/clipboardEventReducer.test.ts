import { describe, expect, it, vi } from 'vitest'
import {
  createInitialClipboardEventReducerState,
  flushClipboardLocal,
  flushClipboardRemote,
  reduceClipboardRealtimeEvent,
  type ClipboardEventReducerAction,
  type ClipboardEventReducerResult,
  type ClipboardEventReducerState,
  type ClipboardRealtimeEvent,
} from '../clipboardEventReducer'

function actionTypes(actions: ClipboardEventReducerAction[]): string[] {
  return actions.map(action => action.type)
}

describe('clipboardEventReducer', () => {
  it('turns incoming pending events into placeholder and transfer-status actions', () => {
    const now = vi.fn(() => 1234)
    const state = createInitialClipboardEventReducerState()

    const result = reduce(
      state,
      {
        topic: 'clipboard',
        eventType: 'clipboard.incoming_pending',
        payload: {
          entryId: 'entry-1',
          fromDevice: 'peer-1',
          totalBytes: 2048,
          filenames: ['report.pdf'],
        },
      },
      now()
    )

    expect(actionTypes(result.actions)).toEqual([
      'clipboard/addPendingEntry',
      'fileTransfer/setEntryTransferStatus',
    ])
    expect(result.actions[0]).toMatchObject({
      payload: {
        entryId: 'entry-1',
        fromDevice: 'peer-1',
        totalBytes: 2048,
        filenames: ['report.pdf'],
        createdAt: 1234,
      },
    })
    expect(result.actions[1]).toMatchObject({
      payload: {
        entryId: 'entry-1',
        status: 'transferring',
        reason: null,
      },
    })
    expect(result.effects).toEqual([])
  })

  it('routes local new content through remove-pending, cancel-write, and immediate local fetch', () => {
    const state = createInitialClipboardEventReducerState()

    const result = reduce(state, newContent('entry-1', 'local'), 1000)

    expect(actionTypes(result.actions)).toEqual([
      'clipboard/removePendingEntry',
      'fileTransfer/cancelClipboardWrite',
    ])
    expect(result.effects).toEqual([{ type: 'flushLocalEntries', entryIds: ['entry-1'] }])
  })

  it('coalesces rapid local new-content events into one trailing fetch effect', () => {
    let state = createInitialClipboardEventReducerState()

    let result = reduce(state, newContent('e1', 'local'), 1000)
    state = result.state
    expect(result.effects).toEqual([{ type: 'flushLocalEntries', entryIds: ['e1'] }])

    result = reduce(state, newContent('e2', 'local'), 1050)
    state = result.state
    expect(result.effects).toEqual([{ type: 'scheduleLocalFlush', delayMs: 250 }])

    result = reduce(state, newContent('e3', 'local'), 1100)
    state = result.state
    expect(result.effects).toEqual([])
    expect(state.pendingLocalEntryIds).toEqual(['e2', 'e3'])

    expect(flushClipboardLocal(state, { nowMs: 1300 })).toMatchObject({
      actions: [],
      effects: [{ type: 'flushLocalEntries', entryIds: ['e2', 'e3'] }],
    })
  })

  it('throttles remote invalidation with a trailing effect', () => {
    let state = createInitialClipboardEventReducerState()

    let result = reduce(state, newContent('remote-1', 'remote'), 1000)
    state = result.state
    expect(result.effects).toEqual([{ type: 'invalidateRemote' }])

    result = reduce(state, newContent('remote-2', 'remote'), 1100)
    state = result.state
    expect(result.effects).toEqual([{ type: 'scheduleRemoteInvalidate', delayMs: 200 }])

    result = reduce(state, newContent('remote-3', 'remote'), 1150)
    state = result.state
    expect(result.effects).toEqual([])
    expect(state.remoteInvalidateScheduled).toBe(true)

    expect(flushClipboardRemote(state, { nowMs: 1300 })).toMatchObject({
      actions: [],
      effects: [{ type: 'invalidateRemote' }],
    })
  })

  it('turns file-transfer status and progress events into store actions', () => {
    const state = createInitialClipboardEventReducerState()

    expect(
      reduce(
        state,
        {
          topic: 'file-transfer',
          eventType: 'file-transfer.status_changed',
          ts: 42,
          payload: {
            transferId: 'tx-1',
            entryId: 'entry-1',
            status: 'cancelled',
            reason: 'cancelled:remote_peer',
          },
        },
        1000
      ).actions
    ).toMatchObject([
      {
        type: 'fileTransfer/linkTransferToEntry',
        payload: { transferId: 'tx-1', entryId: 'entry-1' },
      },
      {
        type: 'fileTransfer/setEntryTransferStatus',
        payload: { entryId: 'entry-1', status: 'cancelled', reason: 'cancelled:remote_peer' },
      },
      { type: 'clipboard/removePendingEntry', payload: 'entry-1' },
      {
        type: 'fileTransfer/markTransferCancelled',
        payload: { transferId: 'tx-1', reason: 'remote_peer' },
      },
    ])

    expect(
      reduce(
        state,
        {
          topic: 'file-transfer',
          eventType: 'file-transfer.progress',
          ts: 99,
          payload: {
            transferId: 'tx-1',
            entryId: 'entry-1',
            peerId: 'peer-1',
            direction: 'Receiving',
            bytesTransferred: 512,
            totalBytes: 1024,
          },
        },
        1000
      ).actions
    ).toMatchObject([
      {
        type: 'fileTransfer/updateTransferProgress',
        payload: {
          transferId: 'tx-1',
          entryId: 'entry-1',
          peerId: 'peer-1',
          direction: 'Receiving',
          bytesTransferred: 512,
          totalBytes: 1024,
          eventTs: 99,
        },
      },
      {
        type: 'fileTransfer/linkTransferToEntry',
        payload: { transferId: 'tx-1', entryId: 'entry-1' },
      },
    ])
  })
})

function reduce(
  state: ClipboardEventReducerState,
  event: ClipboardRealtimeEvent,
  nowMs: number
): ClipboardEventReducerResult {
  return reduceClipboardRealtimeEvent(event, state, { nowMs, throttleMs: 300 })
}

function newContent(entryId: string, origin: 'local' | 'remote'): ClipboardRealtimeEvent {
  return {
    topic: 'clipboard',
    eventType: 'clipboard.new_content',
    payload: { entryId, preview: entryId, origin },
  }
}
