//! 前端 UI 触发的更新事件 → 后端 PostHog facade 的薄包装。
//!
//! 后端 `capture_update_ui_event` Tauri command 接受三类 variant
//! (`dialog_opened` / `dismissed` / `action_invoked`)，对应的字符串枚举
//! 跟 `docs/architecture/telemetry-events.md §7.8` 一致。具体语义：
//!
//! - 所有 capture 都是 fire-and-forget；网络/序列化失败仅 `log.warn`。
//! - `install_kind` 由后端反查注入，前端不传也无法传。
//! - `idle` / `installing` 不属于 schema 的 `UiUpdatePhase` 集合，调用方
//!   遇到时应通过 `toUiPhase` 过滤后再调，或直接跳过 capture。
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
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
    log.warn({ err, event: tag }, 'capture_update_ui_event 失败')
  })
}

export function captureUpdateDialogOpened(source: DialogOpenSource, phase: UiPhase): void {
  fireAndForget(
    commands.captureUpdateUiEvent({ kind: 'dialog_opened', source, phase }),
    'dialog_opened'
  )
}

export function captureUpdateDismissed(phase: UiPhase, source: DismissSource): void {
  fireAndForget(commands.captureUpdateUiEvent({ kind: 'dismissed', phase, source }), 'dismissed')
}

export function captureUpdateActionInvoked(
  action: ActionKind,
  outcome: ActionOutcome,
  errorKind?: string
): void {
  fireAndForget(
    commands.captureUpdateUiEvent({
      kind: 'action_invoked',
      action,
      outcome,
      error_kind: errorKind ?? null,
    }),
    'action_invoked'
  )
}
