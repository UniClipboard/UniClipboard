/**
 * Security API module — typed accessors for daemon encryption endpoints.
 *
 * 安全 API 模块 — daemon 加密端点的类型化访问器。
 *
 * # Daemon Endpoints / Daemon 端点
 * - `GET /encryption/state` → current encryption initialization & session state
 * - `GET /encryption/keychain-access` → verify Keychain "Always Allow" permission
 * - `POST /encryption/unlock` → silent keyring resume (no passphrase)
 * - `POST /encryption/unlock-with-passphrase` → user-driven passphrase unlock
 * - `POST /encryption/factory-reset` → wipe key material + clear setup status
 *
 * # ADR-008 P3-1 / D15 — passphrase now rides loopback
 * Every unlock path (silent + passphrase) and factory-reset goes through the
 * daemon HTTP API via the generated SDK (`callSdk`). The historical
 * "passphrase 不出进程" invariant is RETIRED: the passphrase travels the
 * session-gated loopback request (D14). These wrappers translate the
 * `DaemonApiError` raised by `callSdk` (semantic tag on `.details.code`) back
 * into the typed error unions consumers already switch on, so call sites
 * (`UnlockPage`, `App.tsx`) are unchanged.
 */

import { createLogger } from '@/lib/logger'
import {
  factoryResetSpace as daemonFactoryResetSpace,
  getEncryptionState as daemonGetEncryptionState,
  trySilentUnlock as daemonTrySilentUnlock,
  unlockSpaceWithPassphrase as daemonUnlockSpaceWithPassphrase,
  verifyKeychainAccess as daemonVerifyKeychainAccess,
} from './daemon/encryption'
import { DaemonApiError } from './daemon/errors'
import type { FactoryResetError } from './tauri-command/factory_reset'
import type { UnlockSpaceError } from './tauri-command/space_setup'

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

// Re-export the typed error unions so call sites can pattern-match in catch
// blocks without reaching into the tauri-command layer directly.
export type { UnlockSpaceError } from './tauri-command/space_setup'
export { isUnlockSpaceError } from './tauri-command/space_setup'
export type { FactoryResetError } from './tauri-command/factory_reset'
export { isFactoryResetError } from './tauri-command/factory_reset'

// ── Error translation (loopback DaemonApiError → typed unions) ──

/** Server-emitted semantic codes that map 1:1 to `UnlockSpaceError`. */
const UNLOCK_SPACE_CODES: ReadonlySet<string> = new Set([
  'SETUP_NOT_COMPLETED',
  'SPACE_NOT_INITIALIZED',
  'WRONG_PASSPHRASE',
  'CORRUPTED_KEY_MATERIAL',
  'INTERNAL',
])

/** Server-emitted semantic codes that map 1:1 to `FactoryResetError`. */
const FACTORY_RESET_CODES: ReadonlySet<string> = new Set([
  'KEY_MATERIAL_WIPE_FAILED',
  'STORAGE_FAILED',
  'INTERNAL',
])

/**
 * Read the normalized daemon error body (`{ code, message, details? }`) that
 * `callSdk` parks on `DaemonApiError.details`.
 */
function errorBody(error: unknown): { code?: string; message?: string } | undefined {
  if (error instanceof DaemonApiError) {
    return error.details as { code?: string; message?: string } | undefined
  }
  return undefined
}

function toUnlockSpaceError(error: unknown): UnlockSpaceError {
  const body = errorBody(error)
  const code = body?.code
  if (typeof code === 'string' && UNLOCK_SPACE_CODES.has(code)) {
    return code === 'INTERNAL'
      ? ({ code: 'INTERNAL', message: body?.message ?? 'unlock failed' } as UnlockSpaceError)
      : ({ code } as UnlockSpaceError)
  }
  // Facade not yet assembled surfaces as 503 `runtime_unavailable`.
  if (code === 'runtime_unavailable') {
    return { code: 'FACADE_UNAVAILABLE' } as UnlockSpaceError
  }
  return {
    code: 'INTERNAL',
    message: error instanceof Error ? error.message : String(error),
  } as UnlockSpaceError
}

function toFactoryResetError(error: unknown): FactoryResetError {
  const body = errorBody(error)
  const code = body?.code
  if (typeof code === 'string' && FACTORY_RESET_CODES.has(code)) {
    return { code, message: body?.message ?? '' } as FactoryResetError
  }
  if (code === 'runtime_unavailable') {
    return { code: 'FACADE_UNAVAILABLE' } as FactoryResetError
  }
  return {
    code: 'INTERNAL',
    message: error instanceof Error ? error.message : String(error),
  } as FactoryResetError
}

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

// ── Unlock / reset paths (loopback) ────────────────────────────

/**
 * Silent keyring-only unlock attempt via `POST /encryption/unlock`.
 *
 * 用 keyring 中已存 KEK 静默解锁,不接受 passphrase。
 *
 * @returns `true` if the session was resumed from the keyring,
 *          `false` if there is nothing to resume (no setup yet / keyslot missing).
 * @throws The daemon error when the keyring is *present but mismatches* the
 *          on-disk keyslot, or any other unexpected failure. Callers should
 *          fall back to prompting the user for the passphrase via
 *          {@link unlockSpaceWithPassphrase} when this rejects.
 */
export async function unlockEncryptionSession(): Promise<boolean> {
  try {
    return await daemonTrySilentUnlock()
  } catch (error) {
    log.warn({ err: error }, 'Silent unlock failed — caller should fall back to passphrase prompt')
    throw error
  }
}

/**
 * User-driven passphrase unlock via `POST /encryption/unlock-with-passphrase`
 * (ADR-008 D15 — passphrase rides the session-gated loopback API).
 *
 * 用户主动输入明文口令解锁。
 *
 * After this returns successfully the daemon's `InMemorySession` is ready. The
 * caller must additionally hit `POST /lifecycle/ready` to actually start the
 * daemon's deferred clipboard / sync services — the same step taken after
 * `init` / `redeem`.
 *
 * @throws {UnlockSpaceError} typed union — switch on `error.code`.
 */
export async function unlockSpaceWithPassphrase(passphrase: string): Promise<{ spaceId: string }> {
  try {
    return await daemonUnlockSpaceWithPassphrase(passphrase)
  } catch (error) {
    const typed = toUnlockSpaceError(error)
    // WrongPassphrase is the common user-input case — log at info, not
    // warn/error, to avoid noisy alarms on every retry.
    if (typed.code === 'WRONG_PASSPHRASE') {
      log.info('unlock_space_with_passphrase rejected: wrong passphrase')
    } else {
      log.warn({ code: typed.code }, 'unlock_space_with_passphrase failed')
    }
    throw typed
  }
}

/**
 * 用户主动触发的 "重置并重新开始" —— 删 keyslot + KEK,清 setup_status,
 * 取消所有 pending invitations。经 daemon `POST /encryption/factory-reset`。
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
    await daemonFactoryResetSpace()
  } catch (error) {
    const typed = toFactoryResetError(error)
    log.warn({ code: typed.code }, 'factory_reset_space failed')
    throw typed
  }
}
