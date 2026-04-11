/**
 * Security API module — typed accessors for daemon encryption endpoints.
 *
 * 安全 API 模块 — daemon 加密端点的类型化访问器。
 *
 * # Daemon Endpoints / Daemon 端点
 * - `GET /encryption/state` → current encryption initialization & session state
 * - `POST /encryption/unlock` → auto-unlock encryption session (keyring-based, no passphrase)
 * - `POST /encryption/lock` → lock encryption session (clear master key)
 * - `GET /encryption/keychain-access` → verify Keychain "Always Allow" permission
 */

import {
  getEncryptionState as daemonGetEncryptionState,
  unlockEncryption as daemonUnlockEncryption,
  verifyKeychainAccess as daemonVerifyKeychainAccess,
} from './daemon/encryption'
import { createLogger } from '@/lib/logger'

const log = createLogger('security')

// ── Types ─────────────────────────────────────────────────────

/**
 * Encryption session status.
 *
 * 加密会话状态。
 *
 * Field names match the daemon API response (camelCase).
 */
export interface EncryptionSessionStatus {
  initialized: boolean
  sessionReady: boolean
}

// ── Daemon-based functions ─────────────────────────────────────

/**
 * Fetch encryption session status from the daemon.
 *
 * 从 daemon 获取加密会话状态。
 *
 * Uses daemon HTTP API: `GET /encryption/state`
 *
 * @returns Encryption initialization and session readiness.
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function getEncryptionSessionStatus(): Promise<EncryptionSessionStatus> {
  return daemonGetEncryptionState()
}

/**
 * Auto-unlock the encryption session via the daemon.
 *
 * 通过 daemon 自动解锁加密会话（从 keychain 获取 KEK，无需 passphrase）。
 *
 * Uses daemon HTTP API: `POST /encryption/unlock`
 *
 * @returns True on success, false if encryption not initialized.
 * @throws On unlock errors.
 */
export async function unlockEncryptionSession(): Promise<boolean> {
  try {
    await daemonUnlockEncryption()
    return true
  } catch (error) {
    log.error({ err: error }, 'Failed to unlock encryption session')
    throw error
  }
}

/**
 * Verify macOS Keychain "Always Allow" permission for this app.
 *
 * 验证此应用的 macOS Keychain "始终允许" 权限。
 *
 * Uses daemon HTTP API: `GET /encryption/keychain-access`
 *
 * @returns True if permission is granted.
 * @throws {DaemonApiError} On permission check errors.
 */
export async function verifyKeychainAccess(): Promise<boolean> {
  return daemonVerifyKeychainAccess()
}
