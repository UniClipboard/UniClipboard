/**
 * Upgrade detection API module — typed accessors for daemon upgrade endpoints.
 *
 * 升级检测 API 模块 — daemon 升级检测端点的类型化访问器。
 *
 * # Endpoints / 端点
 * - `GET /upgrade/status` → discriminated upgrade status
 * - `POST /upgrade/ack` → advance the version cursor to the running build
 *
 * # Semantics / 语义
 * The status discriminator `kind` mirrors `uc_application::facade::UpgradeStatus`:
 * - `fresh_install`: profile has never been set up
 * - `no_change`: cursor matches the running build
 * - `upgraded`: build is newer than cursor (or cursor missing on a setup-completed
 *   profile, in which case `from = null`)
 * - `downgraded`: build is older than cursor — the user rolled back
 */

import { daemonClient } from './client'

// ── Wire types ─────────────────────────────────────────────────

export type UpgradeStatus =
  | { kind: 'fresh_install'; current: string }
  | { kind: 'no_change'; current: string }
  | { kind: 'upgraded'; from: string | null; to: string }
  | { kind: 'downgraded'; from: string; to: string }

interface UpgradeStatusEnvelope {
  data: UpgradeStatus
  ts: number
}

interface AckUpgradePayload {
  acknowledged: string
}

interface AckUpgradeEnvelope {
  data: AckUpgradePayload
  ts: number
}

// ── Public API ─────────────────────────────────────────────────

/**
 * Fetch the current upgrade detection status from the daemon.
 *
 * 从 daemon 获取当前升级检测状态。
 *
 * Call once on app startup. Use the returned discriminated value to decide
 * whether to surface the re-pair notice.
 *
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function getUpgradeStatus(): Promise<UpgradeStatus> {
  const res = await daemonClient.request<UpgradeStatusEnvelope>('/upgrade/status')
  return res.data
}

/**
 * Acknowledge the current upgrade — advance the version cursor to the running build.
 *
 * 确认升级 —— 把版本游标推进到当前运行的版本。
 *
 * Idempotent. After a successful call, subsequent `getUpgradeStatus()` calls
 * return `{ kind: 'no_change' }` until the binary version moves.
 *
 * @returns The version string that was written (the daemon's own
 *   `CARGO_PKG_VERSION`, not a value the caller controls).
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function acknowledgeUpgrade(): Promise<string> {
  const res = await daemonClient.request<AckUpgradeEnvelope>('/upgrade/ack', {
    method: 'POST',
  })
  return res.data.acknowledged
}
