//! 前端 UI 触发的更新事件 → daemon `POST /analytics/capture` 的薄包装
//! （ADR-008 D20：daemon 为 product analytics 唯一权威发送方）。
//!
//! 端点接受三类 variant（`dialog_opened` / `dismissed` / `action_invoked`），
//! 对应的字符串枚举跟 `docs/architecture/telemetry-events.md §7.8` 一致。语义：
//!
//! - 所有 capture 都是 fire-and-forget；网络/序列化失败仅 `log.warn`。
//! - `install_kind`（仅 `dialog_opened` 需要）由前端反查 `getInstallKind`
//!   （native，缓存）后透传——拆进程后 daemon 无安装探测代码，运行中 app 的
//!   安装来源由 GUI 原生壳掌握，故由前端供给（见 contract `UiInstallKind` 注释）。
//! - `idle` / `installing` 不属于 schema 的 `UiUpdatePhase` 集合，调用方
//!   遇到时应通过 `toUiPhase` 过滤后再调，或直接跳过 capture。
import { captureUiEvent } from '@/api/daemon/analytics'
import { createLogger } from '@/lib/logger'
import { getInstallKind } from './updater'
import type { DownloadPhase } from './updater'

const log = createLogger('update-telemetry')

export type DialogOpenSource = 'notification' | 'sidebar_icon'
export type DismissSource = 'dialog_later' | 'dialog_closed' | 'package_manager_dialog_closed'
export type UiPhase = 'available' | 'downloading' | 'ready'
export type ActionKind = 'download_bg' | 'install'
export type ActionOutcome = 'started' | 'succeeded' | 'failed' | 'cancelled'

/**
 * 把 UI 层的 `DownloadPhase` 投影到 telemetry schema 接受的子集。
 * 返回 `null` 时调用方应跳过 capture（schema 没有 idle/installing）。
 */
export function toUiPhase(phase: DownloadPhase): UiPhase | null {
  switch (phase) {
    case 'available':
    case 'downloading':
    case 'ready':
      return phase
    default:
      return null
  }
}

function fireAndForget(promise: Promise<unknown>, tag: string): void {
  promise.catch(err => {
    log.warn({ err, event: tag }, 'capture ui event 失败')
  })
}

export function captureUpdateDialogOpened(source: DialogOpenSource, phase: UiPhase): void {
  // `dialog_opened` is the only variant needing install_kind. Probe it natively
  // (cached) and forward; on probe failure fall back to `unknown` so the event
  // still fires rather than being dropped.
  fireAndForget(
    getInstallKind()
      .catch(() => 'unknown' as const)
      .then(installKind =>
        captureUiEvent({ kind: 'dialog_opened', source, phase, install_kind: installKind })
      ),
    'dialog_opened'
  )
}

export function captureUpdateDismissed(phase: UiPhase, source: DismissSource): void {
  fireAndForget(captureUiEvent({ kind: 'dismissed', phase, source }), 'dismissed')
}

export function captureUpdateActionInvoked(
  action: ActionKind,
  outcome: ActionOutcome,
  errorKind?: string
): void {
  fireAndForget(
    captureUiEvent({
      kind: 'action_invoked',
      action,
      outcome,
      error_kind: errorKind ?? null,
    }),
    'action_invoked'
  )
}
