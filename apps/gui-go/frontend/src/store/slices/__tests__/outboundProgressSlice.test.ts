import { describe, expect, it } from 'vitest'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import type { RootState } from '../../index'
import reducer, {
  IDLE_OUTBOUND,
  replaceOutboundSnapshots,
  selectOutboundActivity,
} from '../outboundProgressSlice'

function snapshot(
  entryId: string,
  targetDeviceId: string,
  patch: Partial<OutboundProgressSnapshot> = {}
): OutboundProgressSnapshot {
  return {
    entryId,
    targetDeviceId,
    attemptId: 'attempt-1',
    revision: 1,
    freshness: 'live',
    freshnessAvailable: true,
    state: 'in_progress',
    itemsTotal: 1,
    itemsWaiting: 0,
    itemsRunning: 1,
    itemsCompleted: 0,
    itemsFailed: 0,
    itemsCancelled: 0,
    itemsUnconfirmed: 0,
    items: [],
    ...patch,
  }
}

describe('outboundProgressSlice', () => {
  it('keeps targets of one entry separate', () => {
    const state = reducer(
      undefined,
      replaceOutboundSnapshots([
        snapshot('e1', 'a', { state: 'completed' }),
        snapshot('e1', 'b', { state: 'failed' }),
      ])
    )
    expect(state.byEntry.e1.a.state).toBe('completed')
    expect(state.byEntry.e1.b.state).toBe('failed')
  })

  it('rebuilds from the latest list: absent targets disappear', () => {
    let state = reducer(undefined, replaceOutboundSnapshots([snapshot('e1', 'a')]))
    state = reducer(state, replaceOutboundSnapshots([snapshot('e2', 'b')]))
    expect(state.byEntry.e1).toBeUndefined()
    expect(state.byEntry.e2.b).toBeDefined()
  })

  it('does not let an older revision of the same pair win', () => {
    const state = reducer(
      undefined,
      replaceOutboundSnapshots([
        snapshot('e1', 'a', { revision: 5, state: 'completed' }),
        snapshot('e1', 'a', { revision: 3, state: 'in_progress' }),
      ])
    )
    expect(state.byEntry.e1.a.revision).toBe(5)
    expect(state.byEntry.e1.a.state).toBe('completed')
  })
})

describe('selectOutboundActivity', () => {
  const rootWith = (snapshots: OutboundProgressSnapshot[]) =>
    ({
      outboundProgress: reducer(undefined, replaceOutboundSnapshots(snapshots)),
    }) as unknown as RootState

  it('is idle for unconfirmed and settled snapshots', () => {
    const state = rootWith([
      snapshot('e1', 'a', { state: 'unconfirmed', freshness: 'unconfirmed' }),
      snapshot('e1', 'b', { state: 'completed', freshness: 'settled' }),
    ])
    expect(selectOutboundActivity(state, 'e1')).toBe(IDLE_OUTBOUND)
  })

  it('reports a live running target with an unknown total as null, not 0', () => {
    const state = rootWith([
      snapshot('e1', 'a', {
        items: [{ kind: 'file', index: 0, state: 'transferring', completedBytes: 10 }],
      }),
    ])
    expect(selectOutboundActivity(state, 'e1')).toEqual({ running: true, bytes: 10, total: null })
  })

  it('sums known totals across running targets', () => {
    const item = {
      kind: 'file',
      index: 0,
      state: 'transferring',
      completedBytes: 5,
      totalBytes: 20,
    }
    const state = rootWith([
      snapshot('e1', 'a', { items: [item] }),
      snapshot('e1', 'b', { items: [item] }),
    ])
    expect(selectOutboundActivity(state, 'e1')).toEqual({ running: true, bytes: 10, total: 40 })
  })
})
