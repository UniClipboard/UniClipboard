/**
 * Factory-reset Tauri command wrapper —— "重置并重新开始" 兜底入口。
 *
 * 用户在 `UnlockPage` 上点 "重置并重新开始" 链接 + 走完二次确认后调用本
 * wrapper。语义:删 keyslot + KEK → 清 setup_status → 取消任何 pending
 * invitations。成功后 `EncryptionFacade::state()` 会返回 `initialized=false`,
 * `App.tsx` 的渲染分支自然把 UI 切回 `SetupPage`。
 *
 * Backend: `src-tauri/crates/uc-tauri/src/commands/factory_reset.rs`
 */

import { invokeWithTrace } from '@/lib/tauri-command'

/**
 * 重置成功后返回的占位结果。当前无字段——保留为对象以便未来扩展
 * (例如下一步推荐操作的提示),而不必改 wire 形状。`Record<string, never>`
 * 是 TS 里 "现在空、未来会加字段" 的惯用法 (避免 `{}` 因 ESLint
 * `no-empty-object-type` 报错)。
 */
export type FactoryResetResult = Record<string, never>

/**
 * Factory-reset 失败枚举,镜像 Rust `FactoryResetCommandError`。
 *
 * - `FACADE_UNAVAILABLE`: bootstrap 还没跑完;延后重试或提示用户重启。
 * - `KEY_MATERIAL_WIPE_FAILED`: keyslot / KEK 删除出错。前端应保持
 *   `UnlockPage` 状态而不是跳 SetupPage——残留的 keyslot 会让随后的
 *   init 立即撞 `AlreadyInitialized`。
 * - `STORAGE_FAILED`: key material 已清但 setup_status 未清,UI 处于
 *   过渡态;最稳引导是让用户重启应用。
 * - `INTERNAL`: 兜底,展示通用错误 + Sentry 上报。
 */
export type FactoryResetError =
  | { code: 'FACADE_UNAVAILABLE' }
  | { code: 'KEY_MATERIAL_WIPE_FAILED'; message: string }
  | { code: 'STORAGE_FAILED'; message: string }
  | { code: 'INTERNAL'; message: string }

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
 * **调用前**调用方必须通过二次确认对话框(例如输入 `RESET`)收集用户的
 * 明确意图——本 wrapper 不做任何确认,直接走删除路径。
 *
 * 成功后 `App.tsx` 会随 encryption state 重渲染回 `SetupPage`;调用方
 * 仍应主动把本地的 encryption status 缓存置为 `{ initialized: false,
 * session_ready: false }`,以免等待 state 回流时的短暂闪烁。
 */
export async function factoryResetSpace(): Promise<FactoryResetResult> {
  return await invokeWithTrace<FactoryResetResult>('factory_reset_space')
}
