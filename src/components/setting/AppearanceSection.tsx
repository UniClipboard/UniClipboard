import { type LucideIcon, Check, Monitor, Moon, Sun } from 'lucide-react'
import type { MouseEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingGroup } from './SettingGroup'
import { DEFAULT_THEME_COLOR, THEME_COLORS } from '@/constants/theme'
import { useSetting, type Theme } from '@/hooks/useSetting'
import { setTransitionOrigin } from '@/lib/theme-transition'
import { cn } from '@/lib/utils'

interface ThemeOptionProps {
  value: Theme
  icon: LucideIcon
  label: string
  theme: Theme
  handleThemeChange: (newTheme: Theme, e: MouseEvent) => Promise<void>
}

function ThemeOption({ value, icon: Icon, label, theme, handleThemeChange }: ThemeOptionProps) {
  return (
    <div
      onClick={e => handleThemeChange(value, e)}
      className={cn(
        'cursor-pointer relative flex flex-col items-center gap-2 p-4 rounded-xl border-2 transition-all',
        theme === value
          ? 'border-primary bg-primary/5'
          : 'border-transparent bg-muted/50 hover:bg-muted'
      )}
    >
      <div
        className={cn(
          'p-2 rounded-full',
          theme === value ? 'bg-primary/10 text-primary' : 'bg-transparent text-muted-foreground'
        )}
      >
        <Icon className="w-6 h-6" />
      </div>
      <span
        className={cn(
          'text-sm font-medium',
          theme === value ? 'text-primary' : 'text-muted-foreground'
        )}
      >
        {label}
      </span>
      {theme === value && (
        <div className="absolute top-2 right-2 text-primary">
          <Check className="w-4 h-4" />
        </div>
      )}
    </div>
  )
}

export default function AppearanceSection() {
  const { t } = useTranslation()
  const { setting, updateGeneralSetting } = useSetting()

  // Use derived state instead of local state to avoid initial flash
  const theme = setting?.general?.theme || 'system'

  const handleThemeChange = async (newTheme: Theme, e: MouseEvent) => {
    try {
      setTransitionOrigin(e.clientX, e.clientY)
      await updateGeneralSetting({ theme: newTheme })
    } catch (error) {
      console.error('Failed to change theme:', error)
    }
  }

  const handleThemeColorChange = async (newThemeColor: string, e: MouseEvent) => {
    try {
      setTransitionOrigin(e.clientX, e.clientY)
      await updateGeneralSetting({ theme_color: newThemeColor })
    } catch (error) {
      console.error('Failed to change theme color:', error)
    }
  }

  return (
    <>
      <SettingGroup title={t('settings.sections.appearance.themeMode.title')}>
        <div className="grid grid-cols-3 gap-4 p-4">
          <ThemeOption
            value="light"
            icon={Sun}
            label={t('settings.sections.appearance.themeMode.light')}
            theme={theme}
            handleThemeChange={handleThemeChange}
          />
          <ThemeOption
            value="dark"
            icon={Moon}
            label={t('settings.sections.appearance.themeMode.dark')}
            theme={theme}
            handleThemeChange={handleThemeChange}
          />
          <ThemeOption
            value="system"
            icon={Monitor}
            label={t('settings.sections.appearance.themeMode.system')}
            theme={theme}
            handleThemeChange={handleThemeChange}
          />
        </div>
      </SettingGroup>

      <SettingGroup title={t('settings.sections.appearance.themeColor.title')}>
        <div className="grid grid-cols-4 gap-4 p-4">
          {THEME_COLORS.map(item => (
            <div
              key={item.name}
              data-testid="theme-color-swatch"
              onClick={e => {
                void handleThemeColorChange(item.name, e)
              }}
              className={cn(
                'cursor-pointer group relative flex flex-col items-center gap-2 p-2 rounded-xl border-2 transition-all hover:bg-muted/50',
                setting?.general?.theme_color === item.name ||
                  (item.name === DEFAULT_THEME_COLOR && !setting?.general?.theme_color)
                  ? 'border-primary bg-primary/5'
                  : 'border-transparent'
              )}
            >
              <div className="flex w-full h-6 rounded-lg overflow-hidden shadow-sm">
                {item.previewDots.map((dot, index) => (
                  <span
                    key={index}
                    data-testid="theme-color-dot"
                    className="flex-1 h-full"
                    style={{ backgroundColor: dot }}
                  />
                ))}
              </div>
              <span className="text-xs font-medium capitalize text-muted-foreground group-hover:text-foreground">
                {item.name}
              </span>
            </div>
          ))}
        </div>
      </SettingGroup>
    </>
  )
}
