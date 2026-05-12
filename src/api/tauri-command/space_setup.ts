/**
 * Space setup Tauri command wrappers — passphrase-bearing unlock path.
 *
 * GUI 走 in-process facade 直调 `uc-application::SpaceSetupFacade` /
 * `EncryptionFacade`,不经 daemon webserver。这是承载 **明文口令** 的
 * 唯一前端通路:passphrase 通过 Tauri IPC 直接到同进程的 Rust 入口,
 * **绝不**走 HTTP/TCP socket (避免本机其他进程嗅探 + 减少故障面)。
 *
 * Backend: `src-tauri/crates/uc-tauri/src/commands/space_setup.rs`
 *
 * 设计参考:
 * - `[GUI 走 in-process facade]` 项目原则:webserver 只留给 LAN 业务,
 *   GUI 业务一律直调 facade
 * - `api/tauri-command/mobile_sync.ts` 同模式先例
 */

import { invokeWithTrace } from '@/lib/tauri-command'

// ============================================================================
// Error taxonomy — discriminated unions mirroring Rust side
// ============================================================================

/**
 * 用户口令解锁的失败枚举,镜像 Rust `UnlockSpaceCommandError`。
 *
 * 前端在 catch 块里 `switch (error.code)` 分支处理 UX:
 * - `WRONG_PASSPHRASE`: 提示重输,**保留** modal,不关闭也不清空
 * - `CORRUPTED_KEY_MATERIAL`: 不可恢复,引导用户走 factory reset / 重新 join
 * - `SETUP_NOT_COMPLETED` / `SPACE_NOT_INITIALIZED`: 引导走 init / join 流程
 *   (一般不会触发到这里,因为 UnlockPage 是 setup 完成后才出现的)
 * - `FACADE_UNAVAILABLE`: bootstrap 还没跑完;延后重试或提示重启应用
 * - `INTERNAL`: 兜底,展示通用错误 + Sentry 上报
 */
export type UnlockSpaceError =
  | { code: 'FACADE_UNAVAILABLE' }
  | { code: 'SETUP_NOT_COMPLETED' }
  | { code: 'SPACE_NOT_INITIALIZED' }
  | { code: 'WRONG_PASSPHRASE' }
  | { code: 'CORRUPTED_KEY_MATERIAL' }
  | { code: 'INTERNAL'; message: string }

/**
 * Silent (keyring) unlock 失败枚举,镜像 Rust `TrySilentUnlockError`。
 *
 * `Internal` 覆盖所有 "keyring miss + keyslot 漂移 + 其他非预期";前端
 * 看到 Err 即应当弹 passphrase modal 兜底,而不是把 message 直接展给用户。
 */
export type TrySilentUnlockError =
  | { code: 'FACADE_UNAVAILABLE' }
  | { code: 'INTERNAL'; message: string }

/** Type guard for `unlockSpaceWithPassphrase` rejections. */
export function isUnlockSpaceError(error: unknown): error is UnlockSpaceError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string'
  )
}

/** Type guard for `trySilentUnlock` rejections. */
export function isTrySilentUnlockError(error: unknown): error is TrySilentUnlockError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string'
  )
}

// ============================================================================
// DTOs
// ============================================================================

export interface UnlockSpaceWithPassphraseArgs {
  passphrase: string
}

export interface UnlockSpaceResult {
  spaceId: string
}

export interface TrySilentUnlockResult {
  /**
   * `true` = keyring 命中 + unwrap 成功,session 已 ready;
   * `false` = "没什么可恢复的" (还没 setup / setup 完但 keyslot 缺失);
   * 注意 keyring 与 keyslot **漂移** 会走 reject 路径而不是 `Ok(false)`,
   * 调用方据此区分 "弹解锁 modal" vs "走 init/join 引导"。
   */
  resumed: boolean
}

// ============================================================================
// Command wrappers
// ============================================================================

/**
 * 用户主动输入明文口令解锁 space (in-process)。
 *
 * `passphrase` 仅在 Tauri IPC 边界以明文存在,**绝不**应再次序列化到
 * HTTP/TCP 上 —— `invokeWithTrace` 内置的 Sentry breadcrumb 已通过
 * `@/observability/redaction` 把 `passphrase` 字段脱敏,放心传。
 *
 * 成功后同进程 daemon 的 `InMemorySession` 已 ready;调用方还需触发
 * 一次 daemon `POST /lifecycle/ready` 才能让 clipboard watcher / sync
 * 等 deferred services 真正启动 (现有 init/redeem 路径之后也是这么走的)。
 */
export async function unlockSpaceWithPassphrase(passphrase: string): Promise<UnlockSpaceResult> {
  return await invokeWithTrace<UnlockSpaceResult>('unlock_space_with_passphrase', {
    args: { passphrase },
  })
}

/**
 * Silent keyring resume —— 启动期 auto-unlock + modal 弹出前的探测。
 *
 * In-process 等价于历史 HTTP `POST /encryption/unlock` (不接受 passphrase)。
 * 语义保持原 endpoint 一致: `resumed=true` keyring 命中、`resumed=false`
 * "nothing to resume" (空 profile / 还没 setup)、reject = 异常 / 漂移
 * (调用方应弹 passphrase modal 兜底)。
 */
export async function trySilentUnlock(): Promise<TrySilentUnlockResult> {
  return await invokeWithTrace<TrySilentUnlockResult>('try_silent_unlock')
}
