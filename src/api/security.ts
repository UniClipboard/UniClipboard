/**
 * Security API module — typed accessors for daemon encryption endpoints.
 *
 * 安全 API 模块 — daemon 加密端点的类型化访问器。
 *
 * # Daemon Endpoints (read-only) / Daemon 端点(只读)
 * - `GET /encryption/state` → current encryption initialization & session state
 * - `GET /encryption/keychain-access` → verify Keychain "Always Allow" permission
 *
 * # Tauri Commands (in-process) / Tauri 命令(同进程)
 * - `try_silent_unlock` → silent keyring resume (replaces `POST /encryption/unlock`)
 * - `unlock_space_with_passphrase` → user-driven passphrase unlock (NEW;
 *   only safe path for plaintext passphrase — never crosses HTTP boundary)
 *
 * Migration note: `unlockEncryptionSession` historically wrapped
 * `POST /encryption/unlock`. As of the in-process facade migration it now
 * delegates to the Tauri command `try_silent_unlock` — same semantics
 * (keyring-only, no passphrase) but no longer round-trips through the daemon
 * webserver. Existing call sites do not need to change.
 */

import {
  getEncryptionState as daemonGetEncryptionState,
  verifyKeychainAccess as daemonVerifyKeychainAccess,
} from './daemon/encryption'
import {
  factoryResetSpace as tauriFactoryResetSpace,
  isFactoryResetError,
} from './tauri-command/factory_reset'
import {
  isTrySilentUnlockError,
  isUnlockSpaceError,
  trySilentUnlock as tauriTrySilentUnlock,
  unlockSpaceWithPassphrase as tauriUnlockSpaceWithPassphrase,
} from './tauri-command/space_setup'
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

// Re-export the typed error union so call sites can pattern-match in catch
// blocks without reaching into the tauri-command layer directly.
export type { UnlockSpaceError } from './tauri-command/space_setup'
export { isUnlockSpaceError } from './tauri-command/space_setup'
export type { FactoryResetError } from './tauri-command/factory_reset'
export { isFactoryResetError } from './tauri-command/factory_reset'

// ── Daemon-based read-only functions ───────────────────────────

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

// ── In-process unlock paths ────────────────────────────────────

/**
 * Silent keyring-only unlock attempt (in-process).
 *
 * 用 keyring 中已存 KEK 静默解锁,不接受 passphrase。
 *
 * Replaces the historical `POST /encryption/unlock` HTTP path with a
 * Tauri command invocation — same semantics, but no HTTP round-trip
 * (GUI and daemon share the same `AppFacade` in `GuiInProcess` mode).
 *
 * @returns `true` if the session was resumed from the keyring,
 *          `false` if there is nothing to resume (no setup yet / keyslot missing).
 * @throws An exception when the keyring is *present but mismatches* the
 *          on-disk keyslot, or any other unexpected failure. Callers should
 *          fall back to prompting the user for the passphrase via
 *          {@link unlockSpaceWithPassphrase} when this rejects.
 */
export async function unlockEncryptionSession(): Promise<boolean> {
  try {
    const { resumed } = await tauriTrySilentUnlock()
    return resumed
  } catch (error) {
    if (isTrySilentUnlockError(error)) {
      log.warn(
        { code: error.code },
        'Silent unlock failed — caller should fall back to passphrase prompt'
      )
    } else {
      log.error({ err: error }, 'Silent unlock failed with non-typed error')
    }
    throw error
  }
}

/**
 * User-driven passphrase unlock (in-process, plaintext never leaves the process).
 *
 * 用户主动输入明文口令解锁。
 *
 * Plaintext passphrase is sent over the Tauri IPC boundary only — it never
 * crosses HTTP / TCP socket (which is why this lives on the in-process
 * facade path, not the daemon webserver). The Sentry breadcrumb in
 * `invokeWithTrace` automatically redacts the `passphrase` field.
 *
 * After this returns successfully the in-process daemon's `InMemorySession`
 * is ready. The caller must additionally hit `POST /lifecycle/ready` to
 * actually start the daemon's deferred clipboard / sync services — the
 * same step taken after `init` / `redeem`.
 *
 * @throws {UnlockSpaceError} typed union — switch on `error.code`.
 */
export async function unlockSpaceWithPassphrase(passphrase: string): Promise<{ spaceId: string }> {
  try {
    return await tauriUnlockSpaceWithPassphrase(passphrase)
  } catch (error) {
    if (isUnlockSpaceError(error)) {
      // WrongPassphrase is the common user-input case — log at info, not
      // warn/error, to avoid noisy alarms on every retry. Other variants
      // are genuinely unexpected and worth warn-level logs.
      if (error.code === 'WRONG_PASSPHRASE') {
        log.info('unlock_space_with_passphrase rejected: wrong passphrase')
      } else {
        log.warn({ code: error.code }, 'unlock_space_with_passphrase failed')
      }
    } else {
      log.error({ err: error }, 'unlock_space_with_passphrase failed with non-typed error')
    }
    throw error
  }
}

/**
 * 用户主动触发的 "重置并重新开始" —— 删 keyslot + KEK,清 setup_status,
 * 取消所有 pending invitations。
 *
 * 调用前调用方必须通过二次确认 UI 收集用户的明确意图。成功返回后,
 * `App.tsx` 会随 encryption state 重渲染回 `SetupPage`;为避免短暂闪烁,
 * 调用方应在 await 完成后主动把本地 encryption status 缓存置为
 * `{ initialized: false, session_ready: false }`。
 *
 * @throws {FactoryResetError} typed union — switch on `error.code`。
 */
export async function resetSpace(): Promise<void> {
  try {
    await tauriFactoryResetSpace()
  } catch (error) {
    if (isFactoryResetError(error)) {
      log.warn({ code: error.code }, 'factory_reset_space failed')
    } else {
      log.error({ err: error }, 'factory_reset_space failed with non-typed error')
    }
    throw error
  }
}
