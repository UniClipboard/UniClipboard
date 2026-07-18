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
  listEntryReceiveProgress,
} from '@/api/generated/sdk.gen'
import type { EntryReceiveProgressResponse } from '@/api/generated/types.gen'

export type EntryReceiveProgress = EntryReceiveProgressResponse

export async function listEntryReceives(): Promise<EntryReceiveProgress[]> {
  return daemonClient.callEnveloped(() => listEntryReceiveProgress({ throwOnError: true }))
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
