/**
 * File-transfer Tauri command wrappers.
 *
 * Backend: `src-tauri/crates/uc-tauri/src/commands/file_transfer.rs`
 *
 * Currently exposes only `cancelFileTransfer` — the receiver-side "abort
 * an in-flight inbound transfer" action. The command is idempotent:
 * calling on a `transferId` that has already finished, already been
 * cancelled, or never started succeeds silently. The transfer's final
 * `cancelled` / `failed` status arrives on the next
 * `file-transfer.status_changed` host event; callers should not optimistic-
 * update Redux here.
 */

import { commands } from '@/lib/ipc'

/**
 * Cancel an in-flight inbound file transfer.
 *
 * Reason is hard-coded to `localUser` — this entry point only exists for
 * the user-driven cancel button. Other cancellation reasons (`timeout`,
 * `replaced`, etc.) originate inside the backend and never cross IPC.
 *
 * Rejections map to `CommandError` (Tauri-side); the only currently
 * observable failure is an assembly defect (`InternalError`).
 */
export async function cancelFileTransfer(transferId: string): Promise<void> {
  await commands.cancelFileTransfer(transferId, { tag: 'localUser' })
}
