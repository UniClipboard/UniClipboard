import { Monitor, Moon, Sun } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useSettingSelector } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { setTransitionOrigin } from '@/lib/theme-transition'
import { cn } from '@/lib/utils'

const log = createLogger('theme-mode-switch')

const MODES = [
  { value: 'light', label: 'lightLabel', icon: Sun },
  { value: 'dark', label: 'darkLabel', icon: Moon },
  { value: 'system', label: 'followSystem', icon: Monitor },
] as const

/** Light / Dark / Follow system as a compact segmented radio group, writing the
 * same `general.theme` setting as the Appearance page. */
export function ThemeModeSwitch({ className }: { className?: string }) {
  const { t } = useTranslation()
  const theme = useSettingSelector(context => context.setting?.general?.theme ?? 'system')
  const updateGeneralSetting = useSettingSelector(context => context.updateGeneralSetting)

  return (
    <div
      role="radiogroup"
      aria-label={t('appearanceLayout.theme')}
      className={cn('flex rounded-lg bg-foreground/8 p-0.5', className)}
    >
      {MODES.map(({ value, label, icon: Icon }) => {
        const checked = theme === value
        const name = t(`settings.sections.appearance.themePreview.${label}`)
        return (
          <button
            key={value}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={name}
            title={name}
            onClick={event => {
              if (checked) return
              setTransitionOrigin(event.clientX, event.clientY)
              updateGeneralSetting({ theme: value }).catch(err => {
                log.error({ err }, 'Failed to change theme')
              })
            }}
            className={cn(
              'flex h-6 w-6.5 items-center justify-center rounded-md transition-colors',
              checked
                ? 'bg-background text-foreground shadow-xs'
                : 'text-muted-foreground hover:text-foreground'
            )}
          >
            <Icon className="size-3.5" aria-hidden="true" />
          </button>
        )
      })}
    </div>
  )
}
