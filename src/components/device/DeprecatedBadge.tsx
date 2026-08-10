import { useTranslation } from 'react-i18next'
import { Badge } from '@/components/ui/badge'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'

/**
 * DeprecatedBadge — flags a soon-to-be-removed channel (LAN mobile sync) with
 * a compact "deprecating" pill; hover/focus reveals the full explanation via a
 * tooltip. Wording lives in `devices.mobileSync.deprecated(Tooltip)`.
 *
 * The badge renders through Base UI's render prop so TooltipTrigger can merge
 * its props, which would swallow a DOM ref on the Badge and break hover. Like
 * ExperimentalBadge, the trigger is a transparent native button wrapping the
 * Badge so hover works in both themes.
 */
export function DeprecatedBadge() {
  const { t } = useTranslation()
  const tooltip = t('devices.mobileSync.deprecatedTooltip')

  return (
    <TooltipProvider delay={200}>
      <Tooltip>
        <TooltipTrigger
          render={
            <button
              type="button"
              aria-label={tooltip}
              className="inline-flex cursor-default border-0 bg-transparent p-0"
            />
          }
        >
          <Badge
            variant="outline"
            className="h-4 rounded-full border-border/60 px-1.5 text-[10px] font-medium text-muted-foreground"
          >
            {t('devices.mobileSync.deprecated')}
          </Badge>
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-56 text-left leading-relaxed">
          {tooltip}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}
