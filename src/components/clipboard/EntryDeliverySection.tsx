/**
 * Entry delivery 区段 —— quick-panel 与主窗口 detail 共享渲染。
 *
 * 渲染契约:
 * - "来源" 行: Local / Remote { deviceId } / Historical 三档
 * - "同步状态" 列表: 按 trusted_peer 全集列出每个对端,状态四档 (Delivered /
 *   Duplicate / Pending / Failed),失败附 reason
 * - 边界态:
 *   - Historical: 显示"机制启用前的老 entry,无投递记录" 提示,**不**列设备
 *   - Local + 无 deliveries: 显示"暂未配对任何设备"提示
 *   - delivery 为 null (loading / fetch failed): 渲染骨架或直接隐藏
 *
 * 设备显示名:优先用后端解析后的 `deviceName` / `targetDeviceName`(取自
 * 空间成员目录),不命中时 fallback 到 device_id 前 8 字符截断。
 */

import { Check, CircleDashed, X } from 'lucide-react'
import React, { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  DeliveryFailureReason,
  EntryDeliveryStatusView,
  EntryDeliveryTargetView,
  EntryDeliveryView,
  EntrySourceView,
} from '@/api/tauri-command/clipboard_delivery'
import { cn } from '@/lib/utils'

interface EntryDeliverySectionProps {
  delivery: EntryDeliveryView | null
  /** 紧凑模式:quick-panel 空间窄,字号 / padding 收紧。 */
  compact?: boolean
  className?: string
}

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

const EntryDeliverySection: React.FC<EntryDeliverySectionProps> = ({
  delivery,
  compact = false,
  className,
}) => {
  const { t } = useTranslation()

  // delivery 还没拉回来 / 报错时直接不渲染,避免占位 + 跳动。组件调用者
  // 自己决定要不要给个 loading 骨架。
  if (!delivery) return null

  const isCompact = compact
  const textSize = isCompact ? 'text-[11px]' : 'text-xs'
  const titleSize = isCompact ? 'text-[11px]' : 'text-xs'
  const rowPad = isCompact ? 'py-0.5' : 'py-1'

  return (
    <section
      aria-label={t('delivery.section.aria')}
      className={cn(
        'flex flex-col gap-1.5 border-t border-border/40 px-3 py-2',
        textSize,
        className
      )}
    >
      {/* Source 行 */}
      <SourceLine source={delivery.source} className={titleSize} />

      {/* Deliveries 列表 / Historical / 无 peer 文案 */}
      <DeliveryList delivery={delivery} rowPad={rowPad} textSize={textSize} titleSize={titleSize} />
    </section>
  )
}

interface SourceLineProps {
  source: EntrySourceView
  className?: string
}

const SourceLine: React.FC<SourceLineProps> = ({ source, className }) => {
  const { t } = useTranslation()

  const label = useMemo(() => {
    switch (source.tag) {
      case 'local':
        return t('delivery.source.local')
      case 'remote':
        return t('delivery.source.remote', {
          device: deviceLabel(source.deviceName, source.deviceId),
        })
      case 'historical':
        return t('delivery.source.historical')
    }
  }, [source, t])

  return (
    <div className={cn('flex items-baseline gap-1.5', className)}>
      <span className="shrink-0 text-muted-foreground">{t('delivery.source.label')}</span>
      <span className="min-w-0 truncate font-medium text-foreground">{label}</span>
    </div>
  )
}

interface DeliveryListProps {
  delivery: EntryDeliveryView
  rowPad: string
  textSize: string
  titleSize: string
}

const DeliveryList: React.FC<DeliveryListProps> = ({ delivery, rowPad, textSize, titleSize }) => {
  const { t } = useTranslation()

  // Historical: 老 entry,delivery 表里也是空,显示统一的"无追踪信息"提示。
  if (delivery.source.tag === 'historical') {
    return <p className={cn('text-muted-foreground', textSize)}>{t('delivery.list.historical')}</p>
  }

  // 本机 / 远端 entry,但没有任何 trusted peer (单设备场景)。
  if (delivery.deliveries.length === 0) {
    return <p className={cn('text-muted-foreground', textSize)}>{t('delivery.list.noPeers')}</p>
  }

  return (
    <div className="flex flex-col gap-0.5">
      <span className={cn('text-muted-foreground', titleSize)}>{t('delivery.list.title')}</span>
      <ul className="flex flex-col">
        {delivery.deliveries.map(target => (
          <DeliveryRow
            key={target.targetDeviceId}
            target={target}
            rowPad={rowPad}
            textSize={textSize}
          />
        ))}
      </ul>
    </div>
  )
}

interface DeliveryRowProps {
  target: EntryDeliveryTargetView
  rowPad: string
  textSize: string
}

const DeliveryRow: React.FC<DeliveryRowProps> = ({ target, rowPad, textSize }) => {
  const { t } = useTranslation()
  const icon = renderStatusIcon(target.status)
  const label = renderStatusLabel(target.status, t)
  const tone = renderStatusTone(target.status)

  return (
    <li
      className={cn('flex items-center gap-2 leading-tight', rowPad, textSize)}
      data-status={target.status.tag}
    >
      <span className={cn('shrink-0', tone.icon)} aria-hidden>
        {icon}
      </span>
      <span
        className={cn(
          'min-w-0 flex-1 truncate text-foreground/80',
          // 用了真实名字时不再 monospace —— monospace 留给截断的 device_id。
          target.targetDeviceName && target.targetDeviceName.trim().length > 0 ? '' : 'font-mono'
        )}
      >
        {deviceLabel(target.targetDeviceName, target.targetDeviceId)}
      </span>
      <span className={cn('shrink-0', tone.label)}>{label}</span>
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
      // 已被对端持有但走的是另一路径,弱化展示避免抢眼。
      return { icon: 'text-emerald-500/70', label: 'text-muted-foreground' }
    case 'pending':
      return { icon: 'text-muted-foreground/60', label: 'text-muted-foreground' }
    case 'failed':
      return { icon: 'text-destructive', label: 'text-destructive' }
  }
}

export default EntryDeliverySection
