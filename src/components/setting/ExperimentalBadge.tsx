import { useTranslation } from 'react-i18next'
import { Badge } from '@/components/ui/badge'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'

/**
 * ExperimentalBadge — 标识某个设置项处于实验阶段。
 *
 * Hover/focus 时通过 Tooltip 显示完整说明，提示用户行为可能不稳定。
 * 文案来自 i18n: `devices.settings.badges.experimental(Tooltip)`。
 *
 * 注意:Badge 是无 forwardRef 的 function component,直接 asChild 给 TooltipTrigger
 * 会导致 Radix 拿不到 DOM ref → hover 不触发。这里用原生 span 作为 trigger,
 * 内嵌 Badge 保留视觉样式(同 ConnectionChannelBadge 的处理方式)。
 */
export function ExperimentalBadge() {
  const { t } = useTranslation()
  const tooltip = t('devices.settings.badges.experimentalTooltip')

  return (
    <TooltipProvider delayDuration={200}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span tabIndex={0} aria-label={tooltip} className="inline-flex">
            <Badge variant="secondary">{t('devices.settings.badges.experimental')}</Badge>
          </span>
        </TooltipTrigger>
        <TooltipContent side="top">{tooltip}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

export default ExperimentalBadge
