/**
 * Auth module bridging Tauri bootstrap and daemon HTTP.
 *
 * 连接 Tauri 启动引导与 daemon HTTP 认证的桥接模块。
 *
 * # Responsibilities / 职责
 * - `loadDaemonAuth()`: Poll daemon connection info → initialize DaemonClient → refresh session.
 * - `verifyAuthState()`: Check daemon health (L1) and encryption state (L2).
 * - `waitForEncryptionReady(timeout)`: Poll encryption state until session_ready or timeout.
 */

import { daemonClient } from '@/api/daemon/client'
import { DaemonApiError } from '@/api/daemon/errors'
import type { DaemonConfig, SessionToken } from '@/api/daemon/types'
import { getEncryptionState, getHealth } from '@/api/generated/sdk.gen'
import { waitForDaemonConnectionInfo } from '@/lib/daemon-connection-info'
import { createLogger } from '@/lib/logger'

const log = createLogger('daemon-auth')

/** Default polling interval for waitForEncryptionReady (ms). */
const ENCRYPTION_POLL_INTERVAL_MS = 500

/**
 * Result of `loadDaemonAuth()`.
 *
 * `loadDaemonAuth()` 的返回结果。
 */
export interface DaemonAuthResult {
  /** The session token obtained from the daemon. */
  session: SessionToken
  /** WebSocket URL for subsequent WS connections. */
  wsUrl: string
}

/**
 * Result of `verifyAuthState()`.
 *
 * `verifyAuthState()` 的返回结果。
 */
export interface AuthStateResult {
  /** Whether the daemon /health endpoint responded successfully. */
  daemonReady: boolean
  /** Whether encryption has been initialized (passphrase set). */
  encryptionInitialized: boolean
  /** Whether the encryption session is ready (unlocked). */
  encryptionSessionReady: boolean
}

/**
 * Poll daemon connection info, initialize DaemonClient, and
 * exchange the bearer token for a JWT session.
 *
 * 轮询 daemon 连接信息，初始化 DaemonClient，并用 bearer token 换取 JWT session。
 *
 * @returns Session token and WebSocket URL for downstream consumers.
 * @throws {DaemonApiError} If session refresh fails after initialization.
 */
export async function loadDaemonAuth(): Promise<DaemonAuthResult> {
  const payload = await waitForDaemonConnectionInfo()

  const config: DaemonConfig = {
    baseUrl: payload.baseUrl,
    wsUrl: payload.wsUrl,
  }

  daemonClient.initialize(config)
  const session = await daemonClient.refreshSession()

  return {
    session,
    wsUrl: payload.wsUrl,
  }
}

/**
 * Check daemon reachability (GET /health) and encryption state (GET /encryption/state).
 *
 * 检查 daemon 可达性（GET /health）和加密状态（GET /encryption/state）。
 *
 * @returns Current auth state including daemon health and encryption readiness.
 */
export async function verifyAuthState(): Promise<AuthStateResult> {
  const result: AuthStateResult = {
    daemonReady: false,
    encryptionInitialized: false,
    encryptionSessionReady: false,
  }

  // Step 1: Health check (L1, no auth required).
  // /health is enveloped: { data: { status, ... }, ts } (ADR-008 P2).
  // Call the generated `getHealth` SDK fn DIRECTLY (not via `callSdk`): the
  // health probe runs during bootstrap before a session exists, and `callSdk`
  // would pre-emptively refresh the session, gating this unauthenticated check.
  // `res.data` is the ApiEnvelope; `res.data.data` is the HealthResponse payload.
  try {
    const res = await getHealth({ throwOnError: true })
    result.daemonReady = res.data.data.status === 'ok'
  } catch {
    // Daemon not reachable — return early with all-false state.
    return result
  }

  // Step 2: Encryption state (L2, requires session token). Route through
  // `callSdk` so it drives the session lifecycle; `callSdk` unwraps the SDK's
  // outer `{ data }` to the EncryptionStateEnvelope, whose `.data` is the payload.
  try {
    const envelope = await daemonClient.callSdk(() => getEncryptionState({ throwOnError: true }))
    result.encryptionInitialized = envelope.data.initialized
    result.encryptionSessionReady = envelope.data.sessionReady
  } catch (err) {
    // If encryption state check fails (e.g. 401), daemon is reachable but
    // encryption info is unavailable. daemonReady stays true.
    if (err instanceof DaemonApiError) {
      log.warn({ code: err.code, message: err.message }, 'encryption state check failed')
    }
  }

  return result
}

/**
 * Poll GET /encryption/state every 500ms until `sessionReady === true` or timeout.
 *
 * 每 500ms 轮询 GET /encryption/state，直到 `sessionReady === true` 或超时。
 *
 * @param timeoutMs Maximum time to wait in milliseconds (default: 30000).
 * @returns `true` if encryption became ready, `false` on timeout.
 */
export async function waitForEncryptionReady(timeoutMs = 30_000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs

  while (Date.now() < deadline) {
    try {
      const envelope = await daemonClient.callSdk(() => getEncryptionState({ throwOnError: true }))

      if (envelope.data.sessionReady) {
        return true
      }
    } catch {
      // Ignore transient errors and keep polling until deadline.
    }

    // Wait before next poll, but don't overshoot the deadline.
    const remaining = deadline - Date.now()
    if (remaining <= 0) break
    await sleep(Math.min(ENCRYPTION_POLL_INTERVAL_MS, remaining))
  }

  return false
}

// ── Private helpers ──────────────────────────────────────────────

/**
 * Promise-based sleep utility.
 *
 * 基于 Promise 的延迟工具。
 */
function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms))
}
