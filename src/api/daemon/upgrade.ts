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
 *
 * # Transport / 传输 (ADR-008 P7)
 * Both endpoints route through the @hey-api generated SDK
 * (`getUpgradeStatus` / `acknowledgeUpgrade`) via `daemonClient.callSdk`, which
 * drives the daemon session lifecycle. `callSdk` unwraps the SDK's outer
 * `{ data }` to the canonical `ApiEnvelope { data, ts }`; the payload is then
 * read from `envelope.data`. The public wrapper signatures and the hand-written
 * `UpgradeStatus` domain type below are preserved verbatim for consumers.
 */

import {
  acknowledgeUpgrade as acknowledgeUpgradeSdk,
  getUpgradeStatus as getUpgradeStatusSdk,
} from '@/api/generated/sdk.gen'
import { daemonClient } from './client'

// ── Wire types ─────────────────────────────────────────────────

export type UpgradeStatus =
  | { kind: 'fresh_install'; current: string }
  | { kind: 'no_change'; current: string }
  | { kind: 'upgraded'; from: string | null; to: string }
  | { kind: 'downgraded'; from: string; to: string }

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
  // Route through the generated SDK; `callSdk` unwraps the SDK's `{ data }` to
  // the `UpgradeStatusEnvelope`, then we unwrap `.data` to the payload. The
  // generated `UpgradeStatusDto` is structurally equivalent to the hand-written
  // `UpgradeStatus` discriminated union, bridged here to keep the public return
  // type stable for consumers.
  const envelope = await daemonClient.callSdk(() => getUpgradeStatusSdk({ throwOnError: true }))
  return envelope.data as unknown as UpgradeStatus
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
  // Route through the generated SDK; `callSdk` unwraps the SDK's `{ data }` to
  // the `AckUpgradeEnvelope`, then we read `.data.acknowledged` (the written
  // version string).
  const envelope = await daemonClient.callSdk(() => acknowledgeUpgradeSdk({ throwOnError: true }))
  return envelope.data.acknowledged
}
