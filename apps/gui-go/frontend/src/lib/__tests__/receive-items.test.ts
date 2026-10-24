import { describe, expect, it } from 'vitest'
import {
  isCancellationConfirmed,
  isTerminalAttemptState,
  retryableItemIds,
  retryOutcomeMessageKey,
  summarizeReceiveItems,
} from '../receive-items'
import type { ReceiveSnapshot } from '../receive-items'

function snapshot(state: string, items: ReceiveSnapshot['items']): ReceiveSnapshot {
  return {
    entryId: 'entry-1',
    attemptId: 'attempt-1',
    state,
    totalBytes: 300,
    completedBytes: 100,
    itemsTotal: items.length,
    itemsCompleted: items.filter(item => item.state === 'completed').length,
    items,
  }
}

const file = (itemId: string, state: string, failureReason: string | null = null) => ({
  itemId,
  kind: 'file',
  state,
  failureReason,
})

describe('retryableItemIds', () => {
  it('selects only failed or cancelled items of a settled attempt', () => {
    const failed = snapshot('failed', [
      file('a', 'completed'),
      file('b', 'failed', 'timed_out'),
      file('c', 'cancelled'),
    ])
    expect(retryableItemIds(failed)).toEqual(['b', 'c'])
  })

  it('offers nothing while the attempt is still moving, even if an item already failed', () => {
    for (const state of ['receiving', 'committing', 'cancelling', 'failing']) {
      expect(retryableItemIds(snapshot(state, [file('b', 'failed')]))).toEqual([])
    }
  })

  it('offers nothing for a completed attempt', () => {
    expect(retryableItemIds(snapshot('completed', [file('a', 'completed')]))).toEqual([])
  })

  it('retries a directory as its single unit, never as members', () => {
    const directory = snapshot('failed', [
      { itemId: 'directory_unit', kind: 'directory_unit', state: 'failed', failureReason: null },
    ])
    directory.itemsTotal = 7
    expect(retryableItemIds(directory)).toEqual(['directory_unit'])
  })
})

describe('summarizeReceiveItems', () => {
  it('keeps delivered files counted when a sibling failed', () => {
    const summary = summarizeReceiveItems(
      snapshot('failed', [
        file('a', 'completed'),
        file('b', 'failed', 'storage_unavailable'),
        file('c', 'completed'),
      ])
    )
    expect(summary).toEqual({ saved: 2, failed: 1, cancelled: 0, inProgress: 0, unknown: 0 })
  })

  it('reports an unrecognised item state as unknown instead of guessing', () => {
    const summary = summarizeReceiveItems(snapshot('failed', [file('a', 'quarantined')]))
    expect(summary.unknown).toBe(1)
    expect(summary.saved).toBe(0)
  })

  it('counts pending and transferring items as in progress', () => {
    const summary = summarizeReceiveItems(
      snapshot('receiving', [file('a', 'pending'), file('b', 'transferring')])
    )
    expect(summary.inProgress).toBe(2)
  })
})

describe('attempt and cancellation semantics', () => {
  it('treats only completed, failed and cancelled as terminal', () => {
    for (const state of ['completed', 'failed', 'cancelled']) {
      expect(isTerminalAttemptState(state)).toBe(true)
    }
    for (const state of ['receiving', 'committing', 'cancelling', 'failing', 'surprise']) {
      expect(isTerminalAttemptState(state)).toBe(false)
    }
  })

  it('confirms cancellation only for the exact cancelled outcome', () => {
    expect(isCancellationConfirmed('cancelled')).toBe(true)
    for (const outcome of [
      'cancellation_incomplete',
      'not_receiving',
      'too_late',
      'already_terminal',
      'superseded',
      'something_new',
    ]) {
      expect(isCancellationConfirmed(outcome)).toBe(false)
    }
  })
})

describe('retryOutcomeMessageKey', () => {
  it('does not present an accepted retry as a delivered file', () => {
    expect(retryOutcomeMessageKey({ outcome: 'accepted', attemptId: 'a2', reason: null })).toBe(
      'clipboard.receive.retry.accepted'
    )
  })

  it('maps every business outcome to its own message', () => {
    expect(retryOutcomeMessageKey({ outcome: 'already_receiving' })).toBe(
      'clipboard.receive.retry.alreadyReceiving'
    )
    expect(retryOutcomeMessageKey({ outcome: 'superseded' })).toBe(
      'clipboard.receive.retry.superseded'
    )
    expect(retryOutcomeMessageKey({ outcome: 'cancellation_incomplete' })).toBe(
      'clipboard.receive.retry.cancellationIncomplete'
    )
    expect(retryOutcomeMessageKey({ outcome: 'unavailable', reason: 'source_unavailable' })).toBe(
      'clipboard.receive.retry.sourceUnavailable'
    )
    expect(retryOutcomeMessageKey({ outcome: 'unavailable', reason: 'access_denied' })).toBe(
      'clipboard.receive.retry.accessDenied'
    )
  })

  it('falls back to a neutral message for an outcome this client does not know', () => {
    expect(retryOutcomeMessageKey({ outcome: 'from_the_future' })).toBe(
      'clipboard.receive.retry.unknown'
    )
    expect(retryOutcomeMessageKey({ outcome: 'unavailable', reason: 'new_reason' })).toBe(
      'clipboard.receive.retry.unknown'
    )
  })
})
