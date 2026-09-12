import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import AppearanceThemeWindow from '@/components/setting/appearance/AppearanceThemeWindow'
import type { Theme } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import type { ThemeTokens } from '@/lib/theme-engine'
import { setTransitionOrigin } from '@/lib/theme-transition'

const log = createLogger('appearance-theme')
const OPTIONS = [
  { value: 'system', label: 'followSystem' },
  { value: 'light', label: 'lightLabel' },
  { value: 'dark', label: 'darkLabel' },
] as const
interface Props {
  theme: Theme
  lightTokens: ThemeTokens
  darkTokens: ThemeTokens
  onChange: (theme: Theme) => Promise<void>
}
export default function AppearanceTheme({ theme, lightTokens, darkTokens, onChange }: Props) {
  const { t } = useTranslation()
  const [saving, setSaving] = useState(false)
  const [failed, setFailed] = useState(false)
  const select = async (next: Theme) => {
    setSaving(true)
    setFailed(false)
    try {
      await onChange(next)
    } catch (error) {
      log.error({ err: error }, 'Failed to change theme')
      setFailed(true)
    } finally {
      setSaving(false)
    }
  }
  return (
    <fieldset disabled={saving} className="min-w-0">
      <legend className="mb-4 px-1 text-ui-section font-semibold">
        {t('appearanceLayout.theme')}
      </legend>
      <div className="appearance-theme-choices">
        {OPTIONS.map(({ value, label }) => (
          <label key={value} className="appearance-theme-choice">
            <input
              className="peer sr-only"
              type="radio"
              name="appearance-theme"
              value={value}
              checked={theme === value}
              onClick={event => setTransitionOrigin(event.clientX, event.clientY)}
              onChange={() => {
                void select(value)
              }}
            />
            <span
              aria-hidden="true"
              data-appearance-theme-preview={value}
              className="appearance-theme-preview peer-checked:outline-2 peer-checked:outline-primary peer-focus-visible:outline-2 peer-focus-visible:outline-primary peer-disabled:opacity-60"
            >
              <AppearanceThemeWindow tokens={value === 'dark' ? darkTokens : lightTokens} />
              {value === 'system' && <AppearanceThemeWindow tokens={darkTokens} clipped />}
            </span>
            <span className="appearance-theme-caption text-ui-body peer-checked:font-medium peer-disabled:opacity-60">
              <span
                aria-hidden="true"
                className="appearance-theme-radio"
                data-selected={theme === value}
              />
              <span>{t(`settings.sections.appearance.themePreview.${label}`)}</span>
            </span>
          </label>
        ))}
      </div>
      <p className="mt-4 text-ui-caption-relaxed text-muted-foreground" aria-live="polite">
        {t(`appearanceLayout.themeHelp.${theme}`)}
      </p>
      {failed && (
        <p role="alert" className="mt-2 text-ui-body text-destructive">
          {t('appearanceLayout.saveFailed')}
        </p>
      )}
    </fieldset>
  )
}
