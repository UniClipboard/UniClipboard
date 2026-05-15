/**
 * Space setup Tauri command wrappers — passphrase-bearing unlock path.
 *
 * GUI 走 in-process facade 直调 `uc-application::SpaceSetupFacade` /
 * `EncryptionFacade`，不经 daemon webserver。这是承载 **明文口令** 的
 * 唯一前端通路：passphrase 通过 Tauri IPC 直接到同进程的 Rust 入口，
 * **绝不**走 HTTP/TCP socket（避免本机其他进程嗅探 + 减少故障面）。
 *
 * Backend: `src-tauri/crates/uc-tauri/src/commands/space_setup.rs`
 *
 * 类型来源：`ipc-bindings.generated.ts`（由 `cargo test --test specta_export`
 * 与 Rust 端强制对齐）。本文件只做命名 alias + 类型 guard 适配。
 */

import { commands } from '@/lib/ipc'
import type {
  TrySilentUnlockError,
  TrySilentUnlockResult,
  UnlockSpaceCommandError,
  UnlockSpaceResultDto,
} from '@/lib/ipc'

// ============================================================================
// Error taxonomy — re-exports from generated bindings
// ============================================================================

export type UnlockSpaceError = UnlockSpaceCommandError
export type { TrySilentUnlockError, TrySilentUnlockResult }

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

export type UnlockSpaceResult = UnlockSpaceResultDto

// ============================================================================
// Command wrappers
// ============================================================================

/**
 * 用户主动输入明文口令解锁 space (in-process)。
 *
 * `passphrase` 仅在 Tauri IPC 边界以明文存在，**绝不**应再次序列化到
 * HTTP/TCP 上 —— `commands` proxy 内置的 Sentry breadcrumb 已通过
 * `@/observability/redaction` 把 `passphrase` 字段脱敏，放心传。
 *
 * 成功后同进程 daemon 的 `InMemorySession` 已 ready；调用方还需触发
 * 一次 daemon `POST /lifecycle/ready` 才能让 clipboard watcher / sync
 * 等 deferred services 真正启动（现有 init/redeem 路径之后也是这么走的）。
 */
export async function unlockSpaceWithPassphrase(passphrase: string): Promise<UnlockSpaceResult> {
  return await commands.unlockSpaceWithPassphrase({ passphrase })
}

/**
 * Silent keyring resume —— 启动期 auto-unlock + modal 弹出前的探测。
 *
 * In-process 等价于历史 HTTP `POST /encryption/unlock`（不接受 passphrase）。
 * 语义保持原 endpoint 一致：`resumed=true` keyring 命中、`resumed=false`
 * "nothing to resume"（空 profile / 还没 setup）、reject = 异常 / 漂移
 * （调用方应弹 passphrase modal 兜底）。
 */
export async function trySilentUnlock(): Promise<TrySilentUnlockResult> {
  return await commands.trySilentUnlock()
}
