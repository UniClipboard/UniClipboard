import { Info } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui/popover'

const DISCLOSURE_KEYS = ['pairing', 'discovery', 'telemetry', 'autoUpdate'] as const

/** Explain LAN-only connection behavior and the independently controlled internet requests. */
export function LanOnlyDisclosure() {
  const { t } = useTranslation()

  return (
    <Popover>
      <PopoverTrigger
        render={
          <button
            type="button"
            aria-label={t('settings.sections.network.lanOnly.infoIconAriaLabel')}
            aria-haspopup="dialog"
            className="inline-flex items-center justify-center rounded-md p-1 text-muted-foreground hover:text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          />
        }
      >
        <Info className="size-3.5" aria-hidden="true" />
      </PopoverTrigger>
      <PopoverContent align="start" sideOffset={8} aria-labelledby="lan-only-disclosure-title">
        <div className="space-y-3">
          <div>
            <p id="lan-only-disclosure-title" className="text-ui-body font-medium">
              {t('settings.sections.network.lanOnly.disclosure.title')}
            </p>
            <p className="text-ui-caption text-muted-foreground mt-1">
              {t('settings.sections.network.lanOnly.disclosure.intro')}
            </p>
          </div>
          <div className="space-y-2">
            {DISCLOSURE_KEYS.map(key => (
              <div key={key} className="space-y-1">
                <p className="text-ui-body font-medium">
                  {t(`settings.sections.network.lanOnly.disclosure.${key}.title`)}
                </p>
                <p className="text-ui-caption text-muted-foreground">
                  {t(`settings.sections.network.lanOnly.disclosure.${key}.description`)}
                </p>
              </div>
            ))}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
