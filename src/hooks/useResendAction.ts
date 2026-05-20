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
 * 设计:
 * - hook 提供两个动作: `resendAll(entryId)` 和 `resendToPeer(entryId, deviceId)`,
 *   底层都是同一个 `clipboard_resend_entry` 命令,只是 `targetDeviceIds`
 *   一个传 `null` (差集派生) 一个传 `[deviceId]`。
 * - 并发锁: `entryInFlight` 单 boolean + `peersInFlight: Set<deviceId>`,
 *   两个独立维度。允许 entry-wide 与多个 peer-level 重发同时在飞 (后端
 *   每条命令独立)。
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

import { useCallback, useState } from 'react'
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

export interface UseResendActionResult {
  /** 整 entry 重发是否在飞。 */
  entryInFlight: boolean
  /** 某 peer 单独重发是否在飞 (key = targetDeviceId)。 */
  isPeerInFlight: (deviceId: string) => boolean
  /**
   * 触发整 entry resend (差集派生)。in-flight / 空 entryId 时 noop。
   * 错误已经被 hook 内 toast 吞下,调用方不需要 try/catch。
   */
  resendAll: (entryId: string | null) => Promise<void>
  /**
   * 触发 peer 级 resend。in-flight (该 peer) / 空 entryId 时 noop。
   * 错误同样在 hook 内吞下。
   */
  resendToPeer: (entryId: string | null, deviceId: string) => Promise<void>
}

export function useResendAction(): UseResendActionResult {
  const { t } = useTranslation()
  const [entryInFlight, setEntryInFlight] = useState(false)
  const [peersInFlight, setPeersInFlight] = useState<ReadonlySet<string>>(() => new Set())

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
      if (!entryId || entryInFlight) return
      await fireResend({
        entryId,
        targetDeviceIds: null,
        onStart: () => setEntryInFlight(true),
        onSettle: () => setEntryInFlight(false),
      })
    },
    [entryInFlight, fireResend]
  )

  const resendToPeer = useCallback(
    async (entryId: string | null, deviceId: string) => {
      if (!entryId) return
      if (peersInFlight.has(deviceId)) return
      await fireResend({
        entryId,
        targetDeviceIds: [deviceId],
        onStart: () => {
          setPeersInFlight(prev => {
            const next = new Set(prev)
            next.add(deviceId)
            return next
          })
        },
        onSettle: () => {
          setPeersInFlight(prev => {
            if (!prev.has(deviceId)) return prev
            const next = new Set(prev)
            next.delete(deviceId)
            return next
          })
        },
      })
    },
    [peersInFlight, fireResend]
  )

  const isPeerInFlight = useCallback(
    (deviceId: string) => peersInFlight.has(deviceId),
    [peersInFlight]
  )

  return {
    entryInFlight,
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

function translateResendError(
  err: unknown,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  if (isResendEntryError(err)) {
    const e: ResendEntryCommandError = err
    switch (e.code) {
      case 'ENTRY_NOT_FOUND':
        return t('delivery.resend.error.entryNotFound')
      case 'ENTRY_NOT_RESENDABLE':
        return t(`delivery.resend.error.notResendable.${e.reason}`)
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
