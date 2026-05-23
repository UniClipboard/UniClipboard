/**
 * `useResendAction` —— 触发 `clipboard_resend_entry` + sonner toast 的共享 hook。
 *
 * 为什么把它从 EntryDeliveryBadge 抽出来:
 * commit F 起初把 resend 触发器埋在 `EntryDeliveryBadge` 的 HoverCard popover
 * 里 (用户 hover sync 徽章后才能见到按钮)。真实使用反馈是入口不显眼,
 * 用户期望从 entry 列表项的右键菜单直接触发。所以触发副作用 (调命令 +
 * toast 翻译错误) 与"哪个 UI 元素决定按钮是否 enable / 在飞期间是否
 * disable"两件事必须解耦 —— 右键菜单和 popover 都该共享同一份触发 +
 * toast 逻辑,但 enable 规则不同 (badge 依赖 source / per-peer 状态,
 * 右键菜单信任后端 typed error 做 gate)。
 *
 * 跨 hook 实例的 in-flight 共享 (commit G):
 * 在每个调用 `useResendAction()` 的组件里维护各自独立的 React state 会
 * 让 FileContextMenu 与 EntryDeliveryBadge 对同一 entry 各自计在飞,
 * 用户同时打开右键菜单 + popover 各点一次 Resend 会触发两条 IPC。后端
 * 足够幂等(差集每次重新派生 + dispatch 自身去重)不会脏数据,但 UI 上
 * 会看到两份 success toast,而且第二条命令也消耗资源。把 in-flight
 * 集合提升到模块级单例 + `useSyncExternalStore` 订阅,所有调用点共享
 * 同一份事实,任意 hook 实例发起的请求都能让其他实例的按钮立刻 disable。
 *
 * 设计:
 * - hook 暴露 `isEntryInFlight(entryId)` 与 `isPeerInFlight(entryId, deviceId)`
 *   两个查询 —— 按 entryId 索引模块级集合,UI 据此 disable 按钮。
 * - 并发锁: `entryInFlightSet: Set<entryId>` + `peerInFlightMap: Map<entryId,
 *   Set<deviceId>>`。允许多个 entry 同时在飞、同一 entry 的多个 peer-level
 *   重发同时在飞,但同一 entry-wide 重发与同一 (entry, peer) 不会并发。
 * - 错误翻译: 走 `translateResendError(err, t)`,把 6 类 `error.code` 翻成
 *   i18n 字符串;未知错误兜底 `delivery.resend.error.internal`。
 * - toast 成功: 显示 `{accepted}/{total}` 摘要;`total = accepted + duplicate
 *   + offline + errored + pending`,符合用户视角"我向 N 个对端发了重发"。
 *
 * 调用者协议:
 * - hook 没有 source-aware 守护 —— 调用方若要在 remote/historical entry
 *   上隐藏按钮,自己据 `useEntryDelivery` 判断。后端会拒绝 remote-origin
 *   并返回 `ENTRY_NOT_RESENDABLE.remoteOrigin`,hook 会 toast 告知用户,
 *   即便上层守护漏了也不会留下脏状态。
 */

import { useCallback, useSyncExternalStore } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isResendEntryError,
  resendEntry,
  type ResendEntryCommandError,
  type ResendEntryReportDto,
} from '@/api/tauri-command/clipboard_delivery'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('resend-action')

// ============================================================================
// 模块级 in-flight store —— 所有 useResendAction() 实例共享。
// 使用 number 作为 snapshot,每次 mutation 自增,useSyncExternalStore 据此触
// 发订阅组件重渲。Set / Map 本体保持稳定引用,避免在 mutation 时构造新
// 集合带来的额外 alloc。
// ============================================================================

const entryInFlightSet = new Set<string>()
const peerInFlightMap = new Map<string, Set<string>>()
const listeners = new Set<() => void>()
let snapshotVersion = 0

function getSnapshot(): number {
  return snapshotVersion
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function notify(): void {
  snapshotVersion += 1
  // 拷贝一份监听者列表后再触发 —— 避免某个监听者在 callback 内 unsubscribe
  // 时改 Set 大小导致迭代乱掉(Set 迭代不稳定)。
  for (const listener of Array.from(listeners)) {
    listener()
  }
}

function markEntryStart(entryId: string): void {
  entryInFlightSet.add(entryId)
  notify()
}

function markEntrySettle(entryId: string): void {
  if (entryInFlightSet.delete(entryId)) notify()
}

function markPeerStart(entryId: string, deviceId: string): void {
  let peers = peerInFlightMap.get(entryId)
  if (!peers) {
    peers = new Set<string>()
    peerInFlightMap.set(entryId, peers)
  }
  peers.add(deviceId)
  notify()
}

function markPeerSettle(entryId: string, deviceId: string): void {
  const peers = peerInFlightMap.get(entryId)
  if (!peers || !peers.delete(deviceId)) return
  if (peers.size === 0) peerInFlightMap.delete(entryId)
  notify()
}

/** @internal 测试钩子:清空 in-flight 状态,避免 case 之间相互渗漏。 */
export function __resetResendActionStoreForTests(): void {
  entryInFlightSet.clear()
  peerInFlightMap.clear()
  notify()
}

// ============================================================================
// Hook
// ============================================================================

export interface UseResendActionResult {
  /** 整 entry 重发是否在飞 (空 entryId 时恒 false)。 */
  isEntryInFlight: (entryId: string | null) => boolean
  /** 某 entry 的某 peer 单独重发是否在飞。 */
  isPeerInFlight: (entryId: string | null, deviceId: string) => boolean
  /**
   * 触发整 entry resend (差集派生)。in-flight / 空 entryId 时 noop。
   * 错误已经被 hook 内 toast 吞下,调用方不需要 try/catch。
   */
  resendAll: (entryId: string | null) => Promise<void>
  /**
   * 触发 peer 级 resend。in-flight (该 entry+peer) / 空 entryId 时 noop。
   * 错误同样在 hook 内吞下。
   */
  resendToPeer: (entryId: string | null, deviceId: string) => Promise<void>
}

export function useResendAction(): UseResendActionResult {
  const { t } = useTranslation()
  // 订阅模块级 store:Set / Map 突变时 snapshotVersion++,触发本组件重渲
  // 让 isEntryInFlight / isPeerInFlight 返回最新值。
  useSyncExternalStore(subscribe, getSnapshot, getSnapshot)

  const fireResend = useCallback(
    async (params: {
      entryId: string
      targetDeviceIds: string[] | null
      onStart: () => void
      onSettle: () => void
    }) => {
      params.onStart()
      try {
        const report = await resendEntry({
          entryId: params.entryId,
          targetDeviceIds: params.targetDeviceIds,
        })
        emitResendSuccess(report, t)
      } catch (err) {
        log.warn({ err, entryId: params.entryId }, 'resend entry command failed')
        toast.error(translateResendError(err, t))
      } finally {
        params.onSettle()
      }
    },
    [t]
  )

  const resendAll = useCallback(
    async (entryId: string | null) => {
      if (!entryId) return
      // 共享 store 已记一次 → 任何 hook 实例都视为在飞,直接 noop。
      // 同时检查 peer-level: 若该 entry 的任何 peer 重发在飞,entry-wide
      // 也跳过,避免与单 peer 请求形成重叠派发。
      if (entryInFlightSet.has(entryId)) return
      const peers = peerInFlightMap.get(entryId)
      if (peers && peers.size > 0) return
      await fireResend({
        entryId,
        targetDeviceIds: null,
        onStart: () => markEntryStart(entryId),
        onSettle: () => markEntrySettle(entryId),
      })
    },
    [fireResend]
  )

  const resendToPeer = useCallback(
    async (entryId: string | null, deviceId: string) => {
      if (!entryId) return
      // entry-wide 在飞时,peer 级请求也跳过 —— entry-wide 已覆盖该 peer。
      if (entryInFlightSet.has(entryId)) return
      if (peerInFlightMap.get(entryId)?.has(deviceId)) return
      await fireResend({
        entryId,
        targetDeviceIds: [deviceId],
        onStart: () => markPeerStart(entryId, deviceId),
        onSettle: () => markPeerSettle(entryId, deviceId),
      })
    },
    [fireResend]
  )

  const isEntryInFlight = useCallback(
    (entryId: string | null) => (entryId ? entryInFlightSet.has(entryId) : false),
    []
  )

  const isPeerInFlight = useCallback(
    (entryId: string | null, deviceId: string) =>
      entryId ? (peerInFlightMap.get(entryId)?.has(deviceId) ?? false) : false,
    []
  )

  return {
    isEntryInFlight,
    isPeerInFlight,
    resendAll,
    resendToPeer,
  }
}

function emitResendSuccess(
  report: ResendEntryReportDto,
  t: (key: string, opts?: Record<string, unknown>) => string
) {
  const total =
    report.accepted + report.duplicate + report.offline + report.errored + report.pending
  toast.success(
    t('delivery.resend.success.summary', {
      accepted: report.accepted,
      total,
    })
  )
}

/**
 * 把 entryId 截短给用户看(完整 UUID/ULID 在 toast 里太长会换行,且占了
 * 提示主体空间)。前 8 字符 + 省略号,与 `EntryDeliveryBadge` 里 device id
 * 的 fallback 截断一致。短于阈值时不动,避免误伤本身就短的 fixture。
 */
function shortenEntryId(entryId: string): string {
  if (entryId.length <= 10) return entryId
  return `${entryId.slice(0, 8)}…`
}

function translateResendError(
  err: unknown,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  if (isResendEntryError(err)) {
    const e: ResendEntryCommandError = err
    switch (e.code) {
      case 'ENTRY_NOT_FOUND':
        return t('delivery.resend.error.entryNotFound', {
          entryIdShort: shortenEntryId(e.entryId),
        })
      case 'ENTRY_NOT_RESENDABLE':
        return t(`delivery.resend.error.notResendable.${e.reason}`, {
          entryIdShort: shortenEntryId(e.entryId),
        })
      case 'TARGET_NOT_TRUSTED':
        return t('delivery.resend.error.targetNotTrusted', {
          device: e.deviceId,
        })
      case 'NO_ELIGIBLE_TARGETS':
        return t('delivery.resend.error.noEligibleTargets')
      case 'STORAGE':
      case 'DISPATCH':
        return t('delivery.resend.error.internal', { message: e.message })
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return t('delivery.resend.error.internal', { message })
}
