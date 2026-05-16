/**
 * Entry 投递视图 —— `clipboard_entry_delivery_view` Tauri 命令的前端薄封装。
 *
 * 为什么需要这个模块:
 * 在 entry detail 面板上要渲染"这条剪贴板内容来自哪台设备 / 已经同步到了
 * 哪些可信对端 / 哪些设备失败"。Phase 1 已经把这些视图组装动作沉到
 * `ClipboardSyncFacade::get_entry_delivery_view` 上,这里只是一个跨 IPC 的
 * 薄读封装,供 quick-panel 与主窗口两套 detail 共享调用。
 *
 * 走 in-process facade (Tauri 命令直接通过 `runtime.app_facade()` 调用),
 * 不经 daemon webserver —— GUI 业务一律走 facade 是项目原则。
 *
 * 后端入口: `src-tauri/crates/uc-tauri/src/commands/clipboard_delivery.rs`
 */

import { commands } from '@/lib/ipc'

// ============================================================================
// 视图类型 — 与 Rust 侧 DTO 一一对应。变体名保持小驼峰 (Rust 端 serde
// rename_all = "camelCase"),前端按 `tag` 字段做 discriminated union。
// ============================================================================

/** entry 来源描述。 */
export type EntrySourceView =
  | { tag: 'local' }
  | { tag: 'remote'; deviceId: string }
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
  // tauri-specta 生成的 `EntryDeliveryViewDto` 与本文件手写的 `EntryDeliveryView`
  // 结构同形（字段名 / discriminated union tag literal 完全一致），TS 结构归并
  // 会让它们互通；保留手写类型作为本模块对上层的稳定 API 名称，避免上层
  // (useEntryDelivery / EntryDeliverySection / 测试) 跟随生成文件改名。
  return (await commands.clipboardEntryDeliveryView(entryId)) as EntryDeliveryView
}
