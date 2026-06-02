/**
 * Encryption API module — typed accessors for daemon encryption endpoints.
 *
 * 加密 API 模块 — daemon 加密端点的类型化访问器。
 *
 * # Endpoints / 端点
 * - `GET /encryption/state` → current encryption initialization & session state
 * - `POST /encryption/unlock` → auto-unlock encryption session via keyring (no passphrase)
 * - `POST /encryption/lock` → lock encryption session (clear master key)
 * - `GET /encryption/keychain-access` → verify Keychain "Always Allow" permission
 */

import {
  getEncryptionState as getEncryptionStateSdk,
  lockEncryptionSession as lockEncryptionSessionSdk,
  unlockEncryptionSession as unlockEncryptionSessionSdk,
  verifyKeychainAccess as verifyKeychainAccessSdk,
} from '@/api/generated/sdk.gen'
import { daemonClient } from './client'

// ── Response types ─────────────────────────────────────────────

/**
 * Encryption state as returned by `GET /encryption/state`.
 *
 * 加密状态，由 `GET /encryption/state` 返回。
 *
 * Field names are camelCase to match daemon serde `rename_all = "camelCase"`.
 */
export interface EncryptionStateResponse {
  /** Whether encryption has been initialized (passphrase configured). */
  initialized: boolean
  /** Whether the encryption session is currently unlocked and ready. */
  sessionReady: boolean
}

// ── Public API ─────────────────────────────────────────────────

/**
 * Fetch the current encryption state from the daemon.
 *
 * 从 daemon 获取当前加密状态。
 *
 * @returns Encryption initialization and session readiness.
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function getEncryptionState(): Promise<EncryptionStateResponse> {
  // Route through the generated SDK; `callSdk` unwraps the SDK's outer `{ data }`
  // to the `EncryptionStateEnvelope`, and we unwrap `.data` to the payload. The
  // generated `EncryptionStateResponse` DTO is structurally equivalent to the
  // hand-written interface, bridged here to keep the public return type stable.
  const envelope = await daemonClient.callSdk(() => getEncryptionStateSdk({ throwOnError: true }))
  return envelope.data as unknown as EncryptionStateResponse
}

/**
 * Auto-unlock the encryption session via keyring.
 *
 * 通过 keyring 自动解锁加密会话（无需 passphrase）。
 *
 * Uses daemon HTTP API: `POST /encryption/unlock`
 *
 * @throws {DaemonApiError} On unlock errors (500) or if encryption not initialized.
 */
export async function unlockEncryption(): Promise<void> {
  // Action endpoint: the daemon auto-unlocks via keyring (no passphrase body).
  // The `{ success }` payload is discarded; we only need to surface failures.
  await daemonClient.callSdk(() => unlockEncryptionSessionSdk({ throwOnError: true }))
}

/**
 * Lock the encryption session, clearing the master key from memory.
 *
 * 锁定加密会话，从内存中清除主密钥。
 *
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function lockEncryption(): Promise<void> {
  // Action endpoint: the `{ success }` payload is discarded; only failures matter.
  await daemonClient.callSdk(() => lockEncryptionSessionSdk({ throwOnError: true }))
}

/**
 * Verify macOS Keychain "Always Allow" permission for this app.
 *
 * 验证此应用的 macOS Keychain "始终允许" 权限。
 *
 * Uses daemon HTTP API: `GET /encryption/keychain-access`
 *
 * @returns True if permission is granted.
 * @throws {DaemonApiError} On HTTP or permission check errors.
 */
export async function verifyKeychainAccess(): Promise<boolean> {
  // Route through the generated SDK; `callSdk` unwraps to the
  // `KeychainAccessEnvelope`, then `.data.granted` is the boolean payload.
  const envelope = await daemonClient.callSdk(() => verifyKeychainAccessSdk({ throwOnError: true }))
  return envelope.data.granted
}
