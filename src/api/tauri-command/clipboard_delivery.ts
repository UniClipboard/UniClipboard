/**
 * Entry 投递视图 + 重发 —— daemon loopback API 的前端薄封装。
 *
 * 为什么需要这个模块:
 * 在 entry detail 面板上要渲染"这条剪贴板内容来自哪台设备 / 已经同步到了
 * 哪些可信对端 / 哪些设备失败",以及让用户主动重发。
 *
 * ADR-008 P3-1:改走 daemon HTTP API(生成 SDK + `callSdk`):
 * - `GET /clipboard/entries/{id}/delivery` → `getEntryDeliveryView`
 * - `POST /clipboard/resend` → `resendEntry`
 *
 * 视图类型 + resend 错误/入参类型在本文件 FE-native 定义(in-process Tauri
 * 命令已删除),与 daemon wire 形态(`uc-daemon-contract` DTO + 后端
 * `ResendEntryError` 映射)保持一致。
 */

import { daemonClient } from '@/api/daemon/client'
import { DaemonApiError } from '@/api/daemon/errors'
import {
  getClipboardEntryDelivery as getClipboardEntryDeliverySdk,
  resendClipboardEntry as resendClipboardEntrySdk,
} from '@/api/generated/sdk.gen'

// ============================================================================
// 视图类型 — 与 Rust 侧 DTO 一一对应。变体名保持小驼峰 (Rust 端 serde
// rename_all = "camelCase"),前端按 `tag` 字段做 discriminated union。
// ============================================================================

/** entry 来源描述。 */
export type EntrySourceView =
  | { tag: 'local' }
  | {
      tag: 'remote'
      deviceId: string
      /** 取自空间成员目录;未命中时为 null,渲染层 fallback 到 device_id 截断。 */
      deviceName: string | null
    }
  | { tag: 'historical' }

/** 失败原因细分,与 i18n key `delivery.failureReason.<variant>` 对应。 */
export type DeliveryFailureReason = 'offline' | 'localPolicy' | 'peerRejected' | 'io' | 'internal'

/** 单条投递状态。`Pending` 来自视图层合成 (trusted_peer ∖ 已尝试)。 */
export type EntryDeliveryStatusView =
  | { tag: 'pending' }
  | { tag: 'delivered' }
  | { tag: 'duplicate' }
  | { tag: 'failed'; reason: DeliveryFailureReason }

/** 单个对端的当前同步状态。 */
export interface EntryDeliveryTargetView {
  targetDeviceId: string
  /** 取自空间成员目录;未命中时为 null,渲染层 fallback 到 device_id 截断。 */
  targetDeviceName: string | null
  status: EntryDeliveryStatusView
  /** 失败时的 wire 层错误细节,可选;成功 / Pending 时为 null。 */
  reasonDetail: string | null
  /** `Pending` 时为 null (从未尝试)。 */
  updatedAtMs: number | null
}

/** 完整视图:来源 + 每个可信对端的最新状态。 */
export interface EntryDeliveryView {
  entryId: string
  source: EntrySourceView
  deliveries: EntryDeliveryTargetView[]
}

// ============================================================================
// 调用入口
// ============================================================================

/**
 * 取一条 entry 的"来源 + 每对端同步状态"完整视图。
 *
 * 失败语义:
 * - entry 不存在 (例如刚被删) → Tauri 返回 `NotFound` code,被 reject;
 *   调用方应当把 detail 区域降级为"无投递信息"或直接隐藏 section
 * - facade 未装配 / DB 故障 → `InternalError`,调用方可上报 Sentry +
 *   退化渲染
 *
 * @param entryId 要查询的 entry id (字符串形式,与列表/详情其他 API 一致)
 */
export async function getEntryDeliveryView(entryId: string): Promise<EntryDeliveryView> {
  // ADR-008 P3-1: routes through the generated SDK (`GET /clipboard/entries/{id}/delivery`)
  // instead of the in-process Tauri command. The generated `EntryDeliveryViewDto`
  // is wire-identical (field names + discriminated-union tag literals) to the
  // hand-written `EntryDeliveryView` kept here as the stable API name for upper
  // layers (useEntryDelivery / EntryDeliveryBadge / tests); bridged at the
  // `as unknown as` boundary. `callEnveloped` unwraps down to the payload.
  // EntryNotFound surfaces as a `DaemonApiError` (404) which consumers already
  // degrade-render.
  const data = await daemonClient.callEnveloped(() =>
    getClipboardEntryDeliverySdk({ path: { id: entryId }, throwOnError: true })
  )
  return data as unknown as EntryDeliveryView
}

// ============================================================================
// Resend entry (commit F) ——
// 用户主动重发已存 entry。后端入口:
// `AppFacade::resend_entry` → `ClipboardOutboundFacade::resend_entry` →
// `ResendEntryUseCase`。命令走 in-process facade,不经 daemon HTTP。
// ============================================================================

/** fan-out 后的聚合计数,供 toast 渲染 "{accepted}/{total}" 摘要。 */
export interface ResendEntryReportDto {
  accepted: number
  duplicate: number
  offline: number
  errored: number
  pending: number
}

/** 不可重发的细分原因。i18n key 命名约定:`delivery.resend.error.notResendable.<variant>`。 */
export type NotResendableReasonDto = 'remoteOrigin' | 'payloadLost'

/**
 * typed 错误联合。前端按 `error.code` 做 discriminated union 翻译;
 * camelCase 字段供 i18n 占位 (deviceId / reason / entryId / message)。
 * 与后端 `resend_error_to_response` 的 `code` + `details` 形态一致。
 */
export type ResendEntryCommandError =
  | { code: 'ENTRY_NOT_FOUND'; entryId: string }
  | { code: 'ENTRY_NOT_RESENDABLE'; entryId: string; reason: NotResendableReasonDto }
  | { code: 'TARGET_NOT_TRUSTED'; deviceId: string }
  | { code: 'NO_ELIGIBLE_TARGETS' }
  | { code: 'STORAGE'; message: string }
  | { code: 'DISPATCH'; message: string }

/** 入参 DTO。`targetDeviceIds`: `null`/省略 ⇒ 派生差集;`[ids]` ⇒ 显式 fan-out。 */
export interface ResendEntryArgs {
  entryId: string
  targetDeviceIds?: string[] | null
}

/**
 * Type guard for catch blocks. Tauri rejects with the typed-error envelope
 * shape (`{ code: '...' , ...payload }`); this widens unknown errors back
 * to the typed union so call sites can do `switch (err.code)`.
 *
 * 与 `isMobileSyncError` 同模式;commit E 给 `clipboard_resend_entry`
 * 单独建了 `ResendEntryCommandError` 而非复用 `CommandError`,正是为了
 * 让前端拿到结构化字段做精确的 i18n 文案选择。
 *
 * 收紧到已知 `code` 白名单 —— 其他 typed command 的 error envelope 也是
 * `{ code: string }` 形态,只检查 `typeof code === 'string'` 会把它们误识
 * 别成 resend 错误,触发错误的 i18n key (fallback 到 `internal`)。白名单
 * 必须和 Rust 端 `ResendEntryCommandError` 的 `#[serde(tag = "code")]`
 * 变体名 SCREAMING_SNAKE_CASE 一一对应;扩枚举时同步更新这里 —— 测试
 * `isResendEntryError narrows known codes only` 会守住这条契约。
 */
const RESEND_ERROR_CODES: ReadonlySet<ResendEntryCommandError['code']> = new Set([
  'ENTRY_NOT_FOUND',
  'ENTRY_NOT_RESENDABLE',
  'TARGET_NOT_TRUSTED',
  'NO_ELIGIBLE_TARGETS',
  'STORAGE',
  'DISPATCH',
])

export function isResendEntryError(error: unknown): error is ResendEntryCommandError {
  if (typeof error !== 'object' || error === null || !('code' in error)) {
    return false
  }
  const { code } = error as { code: unknown }
  return typeof code === 'string' && RESEND_ERROR_CODES.has(code as ResendEntryCommandError['code'])
}

/**
 * 重发一条本机 entry。
 *
 * 参数语义:
 * - `entryId` —— 必填。
 * - `targetDeviceIds` —— `null`/省略 ⇒ 后端派生 `trusted_peer \
 *   (Delivered ∪ Duplicate)` 差集;`[id...]` ⇒ 仅向列出设备重发;
 *   `[]` ⇒ 视为零目标,后端返回 `NO_ELIGIBLE_TARGETS`。
 *
 * 失败语义:Tauri reject 抛出的 envelope 形态由 `ResendEntryCommandError`
 * 描述。调用方应当用 `isResendEntryError(err)` 收窄后按 `code` 翻译。
 */
export async function resendEntry(args: ResendEntryArgs): Promise<ResendEntryReportDto> {
  // ADR-008 P3-1: routes through the generated SDK (`POST /clipboard/resend`).
  // `targetDeviceIds` three-state maps onto the `peers` body field
  // (`null`/omitted ⇒ derive diff; `[ids]` ⇒ explicit fan-out). On failure the
  // typed `ResendEntryCommandError` is rebuilt from the normalized error body so
  // the consumers' `error.code` switch + i18n placeholders are unchanged.
  try {
    const data = await daemonClient.callEnveloped(() =>
      resendClipboardEntrySdk({
        body: { entryId: args.entryId, peers: args.targetDeviceIds ?? null },
        throwOnError: true,
      })
    )
    return data as unknown as ResendEntryReportDto
  } catch (error) {
    throw toResendEntryError(error)
  }
}

/**
 * Rebuild the typed `ResendEntryCommandError` from the normalized daemon error
 * body. The resend handler emits the SCREAMING_SNAKE `code` plus the per-variant
 * structured fields (`entryId` / `deviceId` / `reason` / `message`) under
 * `details`, which `callSdk` parks on `DaemonApiError.details`. So the typed
 * error is `{ code, ...details }`. Unknown/transport errors degrade to a generic
 * `DISPATCH` so the UI still shows a resend-failure message.
 */
function toResendEntryError(error: unknown): ResendEntryCommandError {
  if (error instanceof DaemonApiError) {
    const body = error.details as { code?: string; details?: Record<string, unknown> } | undefined
    const code = body?.code
    if (
      typeof code === 'string' &&
      RESEND_ERROR_CODES.has(code as ResendEntryCommandError['code'])
    ) {
      return { code, ...(body?.details ?? {}) } as ResendEntryCommandError
    }
  }
  return {
    code: 'DISPATCH',
    message: error instanceof Error ? error.message : String(error),
  } as ResendEntryCommandError
}
