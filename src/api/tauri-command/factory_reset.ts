/**
 * Factory-reset Tauri command wrapper —— "重置并重新开始" 兜底入口。
 *
 * 用户在 `UnlockPage` 上点 "重置并重新开始" 链接 + 走完二次确认后调用本
 * wrapper。语义：删 keyslot + KEK → 清 setup_status → 取消任何 pending
 * invitations。成功后 `EncryptionFacade::state()` 会返回 `initialized=false`，
 * `App.tsx` 的渲染分支自然把 UI 切回 `SetupPage`。
 *
 * Backend: `src-tauri/crates/uc-tauri/src/commands/factory_reset.rs`
 *
 * 类型来源：`ipc-bindings.generated.ts`（由 `cargo test --test specta_export`
 * 与 Rust 端强制对齐）。
 */

import { commands } from '@/lib/ipc'
import type { FactoryResetCommandError, FactoryResetResult } from '@/lib/ipc'

export type FactoryResetError = FactoryResetCommandError
export type { FactoryResetResult }

/** Type guard for `factoryResetSpace` rejections. */
export function isFactoryResetError(error: unknown): error is FactoryResetError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string'
  )
}

/**
 * 触发 "重置并重新开始"。
 *
 * **调用前**调用方必须通过二次确认对话框（例如输入 `RESET`）收集用户的
 * 明确意图——本 wrapper 不做任何确认，直接走删除路径。
 *
 * 成功后 `App.tsx` 会随 encryption state 重渲染回 `SetupPage`；调用方
 * 仍应主动把本地的 encryption status 缓存置为 `{ initialized: false,
 * session_ready: false }`，以免等待 state 回流时的短暂闪烁。
 */
export async function factoryResetSpace(): Promise<FactoryResetResult> {
  return await commands.factoryResetSpace()
}
