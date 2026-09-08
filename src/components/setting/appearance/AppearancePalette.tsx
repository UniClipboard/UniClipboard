import { Moon, Sun } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import AppearancePaletteSwatch from '@/components/setting/appearance/AppearancePaletteSwatch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui'
import { THEME_COLORS } from '@/constants/theme'
import { createLogger } from '@/lib/logger'
import type { ThemeMode } from '@/lib/theme-engine'
import { setTransitionOrigin } from '@/lib/theme-transition'

const log = createLogger('appearance-palette')
export default function AppearancePalette({
  mode,
  selected,
  onChange,
}: {
  mode: ThemeMode
  selected: string
  onChange: (value: string) => Promise<void>
}) {
  const { t } = useTranslation()
  const [saving, setSaving] = useState(false)
  const [failed, setFailed] = useState(false)
  const label = t(`appearanceLayout.${mode}Palette`)
  const Icon = mode === 'light' ? Sun : Moon
  const select = async (value: string) => {
    setSaving(true)
    setFailed(false)
    try {
      await onChange(value)
    } catch (error) {
      log.error({ err: error }, 'Failed to change palette')
      setFailed(true)
    } finally {
      setSaving(false)
    }
  }
  return (
    <fieldset
      disabled={saving}
      aria-label={label}
      className="appearance-row appearance-palette-row"
    >
      <span className="flex items-center gap-3 text-sm font-normal">
        <Icon className="size-5 shrink-0" aria-hidden="true" />
        {label}
      </span>
      <Select
        value={selected}
        disabled={saving}
        onValueChange={value => {
          void select(value)
        }}
      >
        <SelectTrigger
          aria-label={label}
          className="h-9 w-full min-w-0 text-sm"
          onClick={event => setTransitionOrigin(event.clientX, event.clientY)}
        >
          <SelectValue>
            <AppearancePaletteSwatch name={selected} />
          </SelectValue>
        </SelectTrigger>
        <SelectContent className="max-h-72">
          {THEME_COLORS.map(option => (
            <SelectItem key={option.name} value={option.name} aria-label={option.name}>
              <AppearancePaletteSwatch name={option.name} />
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {failed && (
        <p role="alert" className="appearance-note text-xs text-destructive">
          {t('appearanceLayout.saveFailed')}
        </p>
      )}
    </fieldset>
  )
}
