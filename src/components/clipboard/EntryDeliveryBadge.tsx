/**
 * Entry delivery badge —— 主窗口 detail 与 quick-panel 预览共用的紧凑同步状态展示。
 *
 * 为什么需要这个组件:
 * 同步状态信息(来源 + 每对端投递结果)若按完整列表渲染会独占多行,在
 * detail 顶部显得笨重,在 quick-panel 更会直接压缩内容预览高度。本组件
 * 把这份信息压成两枚 icon (来源 + 同步聚合) + 一句话状态,真正的设备
 * 明细放进 hover popover,两处宿主都能保持单行紧凑。
 *
 * 渲染契约:
 * - 来源 icon: Local / Remote / Historical 三档,tooltip 显示完整文案
 * - 同步聚合 icon + 文字: synced / syncing / partial / failed / pending
 *   - historical 来源 或 无 trusted peer → 不渲染同步部分
 * - popover (hover/click on 同步部分): 列出每个对端 status,行内含失败 reason
 */

import {
  AlertCircle,
  Check,
  CheckCircle2,
  CircleDashed,
  Cloud,
  History,
  Laptop,
  LoaderCircle,
  X,
} from 'lucide-react'
import React, { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  DeliveryFailureReason,
  EntryDeliveryStatusView,
  EntryDeliveryTargetView,
  EntryDeliveryView,
  EntrySourceView,
} from '@/api/tauri-command/clipboard_delivery'
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

interface EntryDeliveryBadgeProps {
  delivery: EntryDeliveryView | null
}

type SyncSummary = 'synced' | 'syncing' | 'partial' | 'failed' | 'pending'

const FAILURE_REASON_KEYS: Record<DeliveryFailureReason, string> = {
  offline: 'delivery.failureReason.offline',
  localPolicy: 'delivery.failureReason.localPolicy',
  peerRejected: 'delivery.failureReason.peerRejected',
  io: 'delivery.failureReason.io',
  internal: 'delivery.failureReason.internal',
}

function truncateDeviceId(deviceId: string): string {
  if (deviceId.length <= 10) return deviceId
  return `${deviceId.slice(0, 8)}…`
}

/** 名字优先于 id:后端解析到真实 name 就用,否则截断 device_id。 */
function deviceLabel(name: string | null | undefined, deviceId: string): string {
  if (name && name.trim().length > 0) return name
  return truncateDeviceId(deviceId)
}

function summarize(targets: readonly EntryDeliveryTargetView[]): SyncSummary | null {
  if (targets.length === 0) return null
  let delivered = 0
  let failed = 0
  let pending = 0
  for (const t of targets) {
    switch (t.status.tag) {
      case 'delivered':
      case 'duplicate':
        delivered += 1
        break
      case 'failed':
        failed += 1
        break
      case 'pending':
        pending += 1
        break
    }
  }
  if (failed === targets.length) return 'failed'
  if (failed > 0) return 'partial'
  if (delivered === targets.length) return 'synced'
  if (delivered > 0 && pending > 0) return 'syncing'
  return 'pending'
}

const EntryDeliveryBadge: React.FC<EntryDeliveryBadgeProps> = ({ delivery }) => {
  const { t } = useTranslation()

  if (!delivery) return null

  const { source, deliveries } = delivery
  // historical 来源 + 空列表 是 legacy entry 的典型形态,展示来源 icon 即可
  // (legacy 无追踪意义)。其它来源即便列表为空也保留来源信息,符合"一眼
  // 看出这条从哪里来"的设计目标。
  const summary = source.tag === 'historical' ? null : summarize(deliveries)

  return (
    <TooltipProvider delayDuration={150}>
      <div className="flex shrink-0 items-center gap-3">
        <SourceBadge source={source} />
        {summary && <SyncBadge summary={summary} deliveries={deliveries} t={t} />}
      </div>
    </TooltipProvider>
  )
}

interface SourceBadgeProps {
  source: EntrySourceView
}

const SourceBadge: React.FC<SourceBadgeProps> = ({ source }) => {
  const { t } = useTranslation()

  const { Icon, label, tone } = useMemo(() => {
    switch (source.tag) {
      case 'local':
        return {
          Icon: Laptop,
          label: t('delivery.source.localShort'),
          tone: 'text-muted-foreground/60',
        }
      case 'remote':
        return {
          Icon: Cloud,
          label: t('delivery.source.remoteShort', {
            device: deviceLabel(source.deviceName, source.deviceId),
          }),
          tone: 'text-sky-500/80',
        }
      case 'historical':
        return {
          Icon: History,
          label: t('delivery.source.historicalShort'),
          tone: 'text-muted-foreground/50',
        }
    }
  }, [source, t])

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="group inline-flex items-center gap-1.5"
          aria-label={label}
          data-source={source.tag}
        >
          <Icon
            className={cn('h-3.5 w-3.5 transition-colors group-hover:text-foreground/80', tone)}
          />
          <span className="text-[11px] font-semibold tabular-nums text-muted-foreground/60 transition-colors group-hover:text-foreground/80">
            {label}
          </span>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top">{label}</TooltipContent>
    </Tooltip>
  )
}

interface SyncBadgeProps {
  summary: SyncSummary
  deliveries: readonly EntryDeliveryTargetView[]
  t: (key: string, opts?: Record<string, unknown>) => string
}

const SyncBadge: React.FC<SyncBadgeProps> = ({ summary, deliveries, t }) => {
  const { Icon, label, tone, spin } = useMemo(() => {
    switch (summary) {
      case 'synced':
        return {
          Icon: CheckCircle2,
          label: t('delivery.summary.synced'),
          tone: 'text-emerald-500',
          spin: false,
        }
      case 'syncing':
        return {
          Icon: LoaderCircle,
          label: t('delivery.summary.syncing'),
          tone: 'text-sky-500',
          spin: true,
        }
      case 'partial':
        return {
          Icon: AlertCircle,
          label: t('delivery.summary.partial'),
          tone: 'text-amber-500',
          spin: false,
        }
      case 'failed':
        return {
          Icon: AlertCircle,
          label: t('delivery.summary.failed'),
          tone: 'text-destructive',
          spin: false,
        }
      case 'pending':
        return {
          Icon: CircleDashed,
          label: t('delivery.summary.pending'),
          tone: 'text-muted-foreground/70',
          spin: false,
        }
    }
  }, [summary, t])

  // HoverCard 原生处理 hover 行为:trigger ↔ content 互相 hover 时不会进入
  // 关闭流程,跨越间隙也不会触发 close → open 的闪烁。
  return (
    <HoverCard>
      <HoverCardTrigger asChild>
        <button
          type="button"
          aria-label={t('delivery.popover.ariaTrigger')}
          className="group inline-flex items-center gap-1.5 rounded outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
          data-summary={summary}
        >
          <Icon className={cn('h-3.5 w-3.5 transition-colors', tone, spin && 'animate-spin')} />
          <span
            className={cn(
              'text-[11px] font-semibold tabular-nums transition-colors',
              tone,
              'opacity-80 group-hover:opacity-100'
            )}
          >
            {label}
          </span>
        </button>
      </HoverCardTrigger>
      <HoverCardContent align="end" side="bottom" sideOffset={6} className="w-64 p-2">
        <div className="mb-1 px-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground/80">
          {t('delivery.popover.title')}
        </div>
        <ul className="flex flex-col">
          {deliveries.map(target => (
            <DeliveryRow key={target.targetDeviceId} target={target} />
          ))}
        </ul>
      </HoverCardContent>
    </HoverCard>
  )
}

interface DeliveryRowProps {
  target: EntryDeliveryTargetView
}

const DeliveryRow: React.FC<DeliveryRowProps> = ({ target }) => {
  const { t } = useTranslation()
  const tone = renderStatusTone(target.status)

  return (
    <li
      className="flex items-center gap-2 px-1 py-1 text-[11px] leading-tight"
      data-status={target.status.tag}
    >
      <span className={cn('shrink-0', tone.icon)} aria-hidden>
        {renderStatusIcon(target.status)}
      </span>
      <span
        className={cn(
          'min-w-0 flex-1 truncate text-foreground/80',
          target.targetDeviceName && target.targetDeviceName.trim().length > 0 ? '' : 'font-mono'
        )}
      >
        {deviceLabel(target.targetDeviceName, target.targetDeviceId)}
      </span>
      <span className={cn('shrink-0', tone.label)}>{renderStatusLabel(target.status, t)}</span>
    </li>
  )
}

function renderStatusIcon(status: EntryDeliveryStatusView) {
  switch (status.tag) {
    case 'delivered':
    case 'duplicate':
      return <Check className="h-3 w-3" />
    case 'pending':
      return <CircleDashed className="h-3 w-3" />
    case 'failed':
      return <X className="h-3 w-3" />
  }
}

function renderStatusLabel(
  status: EntryDeliveryStatusView,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  switch (status.tag) {
    case 'delivered':
      return t('delivery.status.delivered')
    case 'duplicate':
      return t('delivery.status.duplicate')
    case 'pending':
      return t('delivery.status.pending')
    case 'failed':
      return t('delivery.status.failedWithReason', {
        reason: t(FAILURE_REASON_KEYS[status.reason]),
      })
  }
}

interface StatusTone {
  icon: string
  label: string
}

function renderStatusTone(status: EntryDeliveryStatusView): StatusTone {
  switch (status.tag) {
    case 'delivered':
      return { icon: 'text-emerald-500', label: 'text-foreground/80' }
    case 'duplicate':
      return { icon: 'text-emerald-500/70', label: 'text-muted-foreground' }
    case 'pending':
      return { icon: 'text-muted-foreground/60', label: 'text-muted-foreground' }
    case 'failed':
      return { icon: 'text-destructive', label: 'text-destructive' }
  }
}

export default EntryDeliveryBadge
