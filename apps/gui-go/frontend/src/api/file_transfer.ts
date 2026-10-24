/**
 * File-transfer API wrappers.
 *
 * The daemon owns inbound transfer state. UI callers only request a local-user
 * cancellation and then wait for the next file-transfer status event.
 */

import { daemonClient } from '@/api/daemon/client'
import {
  cancelClipboardTransfer,
  cancelEntryReceive as cancelEntryReceiveSdk,
  getEntryReceiveProgress,
  listEntryReceiveProgress,
  retryEntryReceive as retryEntryReceiveSdk,
} from '@/api/generated/sdk.gen'
import type { EntryReceiveProgressResponse } from '@/api/generated/types.gen'
import type { ReceiveRetryOutcome, ReceiveSnapshot } from '@/lib/receive-items'

export type EntryReceiveProgress = EntryReceiveProgressResponse

export async function listEntryReceives(): Promise<EntryReceiveProgress[]> {
  return daemonClient.callEnveloped(() => listEntryReceiveProgress({ throwOnError: true }))
}

/**
 * Read the authoritative receive snapshot of one entry, including the per-item
 * results. Events only say that something changed; this is where state is read.
 */
export async function getEntryReceive(entryId: string): Promise<ReceiveSnapshot | null> {
  // The envelope's `data` is absent when the entry has no receive.
  const envelope = await daemonClient.callSdk(() =>
    getEntryReceiveProgress({ path: { id: entryId }, throwOnError: true })
  )
  const snapshot = envelope.data
  if (!snapshot) return null
  // A daemon that predates per-item results omits `items`.
  return { ...snapshot, items: snapshot.items ?? [] }
}

/**
 * Ask the Engine to retry the selected failed or cancelled items.
 *
 * `itemIds` must come from the latest {@link getEntryReceive} result. `accepted`
 * only means a new attempt was admitted; the items are saved when a later
 * snapshot says so.
 */
export async function retryEntryReceive(
  entryId: string,
  expectedAttemptId: string,
  itemIds: string[]
): Promise<ReceiveRetryOutcome> {
  if (itemIds.length === 0) throw new Error('retryEntryReceive requires at least one item id')
  return daemonClient.callEnveloped(() =>
    retryEntryReceiveSdk({
      path: { id: entryId },
      body: { expectedAttemptId, itemIds },
      throwOnError: true,
    })
  )
}

export async function cancelEntryReceive(entryId: string, attemptId: string): Promise<string> {
  const response = await daemonClient.callEnveloped(() =>
    cancelEntryReceiveSdk({
      path: { id: entryId },
      body: { attemptId },
      throwOnError: true,
    })
  )
  return response.outcome
}

/**
 * Cancel an in-flight inbound file transfer.
 *
 * Routes through the @hey-api generated SDK + `daemonClient.callSdk` (ADR-008 P7)
 * so the `{ data, ts }` envelope is unwrapped and the daemon session lifecycle
 * applies. The cancellation outcome is intentionally discarded — the daemon
 * treats missing or already-finished transfers as idempotent success, and the UI
 * just waits for the next file-transfer status event.
 */
export async function cancelFileTransfer(transferId: string): Promise<void> {
  await daemonClient.callSdk(() =>
    cancelClipboardTransfer({
      path: { transfer_id: transferId },
      body: { reason: 'local_user' },
      throwOnError: true,
    })
  )
}
