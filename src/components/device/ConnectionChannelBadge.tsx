/**
 * ConnectionChannelBadge — Phase 96 INDIC-01.
 *
 * 显示某台已配对设备当前的连接通道:直连 / 中转 / 未连接 /
 * Unknown / 不在本地网络(灰态)。
 *
 * ## 5 态合成规则
 *
 * 后端只产 4 态(`direct/relay/offline/unknown`),"不在本地网络" 是 UI 层
 * `channel + (allowRelayFallback === false)` 的合成态:**LAN-only Mode = ON**
 * 且对端 `channel ∈ {relay, offline}` 时,UI 把它渲染成灰色 + tooltip
 * 解释,而不是让 infra 生造第五个枚举值。设计依据:Pitfall 4 "通道单一真相源
 * 在 infra,UI 不基于 IP 段推断"——本组件不查 setting 之外的任何状态。
 *
 * ## 视觉设计
 *
 * 此前实现使用 Badge + 图标包装,在卡片里和"在线/离线"状态分两行显示。
 * 用户反馈两行展示信息密度低、视觉割裂,改为单行内联文字 + `·` 分隔
 * (在 `SpaceMembersPanel` 内拼装)。这里只产出 inline 文本 + tooltip。
 *
 * ## 不在本组件里
 *
 * * 直接订阅 `peers.changed` —— 由 `SpaceMembersPanel` 拉取传入,本组件纯展示
 * * 5–15s polling —— 由 `DevicesPage` 顶层定时器驱动,本组件不重复
 * * 反向命名翻译 —— `allowRelayFallback === false ⇔ LAN-only ON`,父组件传
 *   `lanOnlyActive: boolean` 进来,这里只看一次合成
 */

import React from 'react'
import { useTranslation } from 'react-i18next'
import type { ConnectionChannel } from '@/api/daemon/members'
import {
  deriveBadgeKind,
  type DerivedBadgeKind,
} from '@/components/device/connection-channel-utils'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'

interface ConnectionChannelBadgeProps {
  channel: ConnectionChannel
  /** LAN-only Mode 是否已开启(后端 `allowRelayFallback === false`)。 */
  lanOnlyActive: boolean
  className?: string
}

const KIND_TEXT_CLASS: Record<DerivedBadgeKind, string> = {
  lan: 'text-emerald-600 dark:text-emerald-400',
  relay: 'text-amber-600 dark:text-amber-400',
  offline: 'text-muted-foreground',
  unknown: 'text-muted-foreground',
  outOfLan: 'text-muted-foreground',
}

const ConnectionChannelBadge: React.FC<ConnectionChannelBadgeProps> = ({
  channel,
  lanOnlyActive,
  className,
}) => {
  const { t } = useTranslation()
  const kind = deriveBadgeKind(channel, lanOnlyActive)
  const textClass = KIND_TEXT_CLASS[kind]

  const label = t(`devices.list.channel.${kind}`)
  const tip = t(`devices.list.channel.tooltip.${kind}`)

  return (
    <TooltipProvider delayDuration={200}>
      <Tooltip>
        {/*
          用原生 span 当 trigger,既保留可 hover 语义,又让 Radix 拿到真实
          DOM ref(避免 Function components cannot be given refs 警告)。
          外层 SpaceMembersPanel 已是 button,这里不再加 tabIndex/role,
          以免造成 button 嵌套的 a11y 冲突。
        */}
        <TooltipTrigger asChild>
          <span aria-label={tip} className={`${textClass} ${className ?? ''}`}>
            {label}
          </span>
        </TooltipTrigger>
        <TooltipContent side="top">{tip}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

export default ConnectionChannelBadge
