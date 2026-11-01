/**
 * Receiver-side per-item receive results.
 *
 * The Engine owns attempts, retries and cancellation. This module only
 * interprets the daemon snapshot for display and for building a retry request.
 */

import type { EntryReceiveProgressResponse, ReceiveItemResponse } from '@/api/generated/types.gen'

export type ReceiveItem = ReceiveItemResponse
export type ReceiveSnapshot = EntryReceiveProgressResponse & { items: ReceiveItem[] }

export interface ReceiveRetryOutcome {
  outcome: string
  attemptId?: string | null
  reason?: string | null
}

export interface ReceiveItemSummary {
  saved: number
  failed: number
  cancelled: number
  inProgress: number
  unknown: number
}

const TERMINAL_ATTEMPT_STATES = new Set(['completed', 'failed', 'cancelled'])
const RETRYABLE_ITEM_STATES = new Set(['failed', 'cancelled'])
const RETRYABLE_ATTEMPT_STATES = new Set(['failed', 'cancelled'])

/** Only `completed`, `failed` and `cancelled` end an attempt. Unknown states stay open. */
export function isTerminalAttemptState(state: string): boolean {
  return TERMINAL_ATTEMPT_STATES.has(state)
}

/**
 * Item ids that may be retried: failed or cancelled items of an attempt that has
 * settled as failed or cancelled. The ids come from the snapshot, never from names.
 */
export function retryableItemIds(snapshot: ReceiveSnapshot): string[] {
  if (!RETRYABLE_ATTEMPT_STATES.has(snapshot.state)) return []
  return snapshot.items.filter(item => RETRYABLE_ITEM_STATES.has(item.state)).map(i => i.itemId)
}

export function summarizeReceiveItems(snapshot: ReceiveSnapshot): ReceiveItemSummary {
  const summary: ReceiveItemSummary = {
    saved: 0,
    failed: 0,
    cancelled: 0,
    inProgress: 0,
    unknown: 0,
  }
  for (const item of snapshot.items) {
    if (item.state === 'completed') summary.saved += 1
    else if (item.state === 'failed') summary.failed += 1
    else if (item.state === 'cancelled') summary.cancelled += 1
    else if (item.state === 'pending' || item.state === 'transferring') summary.inProgress += 1
    else summary.unknown += 1
  }
  return summary
}

/** Only the exact `cancelled` outcome confirms the receive settled as cancelled. */
export function isCancellationConfirmed(outcome: string): boolean {
  return outcome === 'cancelled'
}

/**
 * Message key for a retry outcome. `accepted` only says another attempt was
 * admitted; whether a file was saved is decided by the next snapshot.
 */
export function retryOutcomeMessageKey(result: ReceiveRetryOutcome): string {
  switch (result.outcome) {
    case 'accepted':
      return 'clipboard.receive.retry.accepted'
    case 'already_receiving':
      return 'clipboard.receive.retry.alreadyReceiving'
    case 'superseded':
      return 'clipboard.receive.retry.superseded'
    case 'cancellation_incomplete':
      return 'clipboard.receive.retry.cancellationIncomplete'
    case 'unavailable':
      if (result.reason === 'source_unavailable') return 'clipboard.receive.retry.sourceUnavailable'
      if (result.reason === 'access_denied') return 'clipboard.receive.retry.accessDenied'
      return 'clipboard.receive.retry.unknown'
    default:
      return 'clipboard.receive.retry.unknown'
  }
}
