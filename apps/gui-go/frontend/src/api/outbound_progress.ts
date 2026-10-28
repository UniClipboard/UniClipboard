/**
 * Sender-side outbound progress wrappers.
 *
 * The Engine owns the per (entry, target device) snapshot. Events only say that
 * something changed (`file-transfer.outbound_progress_changed`); this is where
 * state is read, in full.
 */

import { daemonClient } from '@/api/daemon/client'
import { listOutboundProgress } from '@/api/generated/sdk.gen'
import type { OutboundProgressResponse } from '@/api/generated/types.gen'

export type OutboundProgressSnapshot = OutboundProgressResponse

/** Every retained outbound snapshot, one per (entry, target device). */
export async function listOutboundSnapshots(): Promise<OutboundProgressSnapshot[]> {
  return daemonClient.callEnveloped(() => listOutboundProgress({ throwOnError: true }))
}
