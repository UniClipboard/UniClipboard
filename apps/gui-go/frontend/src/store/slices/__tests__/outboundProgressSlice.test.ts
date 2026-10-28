import { describe, expect, it } from 'vitest'
import type { OutboundProgressSnapshot } from '@/api/outbound_progress'
import reducer, { replaceOutboundSnapshots } from '../outboundProgressSlice'

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
