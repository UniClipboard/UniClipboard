import { ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import AppearanceColorPicker from '@/components/setting/appearance/AppearanceColorPicker'
import AppearanceDisplay from '@/components/setting/appearance/AppearanceDisplay'
import AppearancePalette from '@/components/setting/appearance/AppearancePalette'
import AppearanceTheme from '@/components/setting/appearance/AppearanceTheme'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { SettingRow } from '@/components/setting/SettingRow'
import SmoothModeSetting from '@/components/setting/SmoothModeSetting'
import { Switch } from '@/components/ui/switch'
import { DEFAULT_THEME_COLOR } from '@/constants/theme'
import { useOmarchyTheme } from '@/hooks/useOmarchyTheme'
import { useSetting } from '@/hooks/useSetting'
import { themePresets, type OverridableToken, type ThemeMode } from '@/lib/theme-engine'
import '@/components/setting/appearance/appearance-layout.css'

const COLOR_FIELDS: { token: OverridableToken; label: string }[] = [
  { token: 'primary', label: 'accent' },
  { token: 'background', label: 'background' },
  { token: 'foreground', label: 'foreground' },
  { token: 'border', label: 'border' },
]
const MODES: ThemeMode[] = ['light', 'dark']

export default function AppearanceSection() {
  const { t } = useTranslation()
  const { setting, updateGeneralSetting } = useSetting()
  const omarchy = useOmarchyTheme()
  const general = setting?.general
  const presets = {
    light: general?.themeColorLight || general?.themeColor || DEFAULT_THEME_COLOR,
    dark: general?.themeColorDark || general?.themeColor || DEFAULT_THEME_COLOR,
  }
  const overrides = {
    light: general?.themeOverridesLight ?? {},
    dark: general?.themeOverridesDark ?? {},
  }
  const updateColor = (mode: ThemeMode, token: OverridableToken, value: string | null) => {
    const next = { ...overrides[mode] }
    if (value === null) delete next[token]
    else next[token] = value
    return updateGeneralSetting(
      mode === 'light' ? { themeOverridesLight: next } : { themeOverridesDark: next }
    )
  }
  return (
    <div className="appearance-settings min-w-0" data-testid="appearance-settings">
      <div className="flex min-w-0 flex-col gap-8">
        {(omarchy.available || omarchy.enabled) && (
          <div>
            <SettingRow
              label={t('omarchyTheme.title')}
              description={t(
                omarchy.available ? 'omarchyTheme.description' : 'omarchyTheme.unavailable'
              )}
            >
              <Switch
                aria-label={t('omarchyTheme.title')}
                checked={omarchy.enabled}
                disabled={omarchy.saving}
                onCheckedChange={enabled => {
                  void omarchy.setEnabled(enabled)
                }}
              />
            </SettingRow>
            {omarchy.failed && (
              <p role="alert" className="text-ui-body text-destructive">
                {t('appearanceLayout.saveFailed')}
              </p>
            )}
          </div>
        )}
        <fieldset
          disabled={omarchy.enabled}
          className="min-w-0 flex flex-col gap-8 disabled:opacity-60"
        >
          <legend className="sr-only">{t('appearanceLayout.theme')}</legend>
          <div inert={omarchy.enabled || undefined} className="min-w-0 flex flex-col gap-8">
            <div className="min-w-0">
              <AppearanceTheme
                theme={general?.theme || 'system'}
                lightTokens={{
                  ...(themePresets[presets.light] ?? themePresets[DEFAULT_THEME_COLOR]).light,
                  ...overrides.light,
                }}
                darkTokens={{
                  ...(themePresets[presets.dark] ?? themePresets[DEFAULT_THEME_COLOR]).dark,
                  ...overrides.dark,
                }}
                onChange={theme => updateGeneralSetting({ theme })}
              />
            </div>
            <SettingGroup title={t('appearanceLayout.palette')}>
              <div>
                <div className="appearance-palette-list">
                  {MODES.map(mode => (
                    <AppearancePalette
                      key={mode}
                      mode={mode}
                      selected={presets[mode]}
                      onChange={value =>
                        updateGeneralSetting(
                          mode === 'light'
                            ? { themeColorLight: value, themeColor: null }
                            : { themeColorDark: value, themeColor: null }
                        )
                      }
                    />
                  ))}
                </div>
                <details className="group mt-1 pl-3">
                  <summary className="flex w-fit cursor-pointer list-none items-center gap-3 rounded-sm py-2 text-ui-body text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
                    <ChevronRight aria-hidden="true" className="size-4 group-open:rotate-90" />
                    {t('appearanceLayout.customColors')}
                  </summary>
                  <div className="mt-3 grid gap-5 sm:grid-cols-2">
                    {MODES.map(mode => (
                      <fieldset key={mode} className="min-w-0">
                        <legend className="mb-1 text-ui-section ">
                          {t(`appearanceLayout.${mode}Palette`)}
                        </legend>
                        <div className="divide-y divide-border/25">
                          {COLOR_FIELDS.map(({ token, label }) => (
                            <AppearanceColorPicker
                              key={`${omarchy.enabled}:${presets[mode]}:${token}`}
                              label={t(`settings.sections.appearance.${mode}Theme.${label}`)}
                              presetColor={
                                (themePresets[presets[mode]] ?? themePresets[DEFAULT_THEME_COLOR])[
                                  mode
                                ][token]
                              }
                              overrideColor={overrides[mode][token] ?? null}
                              onChange={value => updateColor(mode, token, value)}
                            />
                          ))}
                        </div>
                      </fieldset>
                    ))}
                  </div>
                </details>
              </div>
            </SettingGroup>
          </div>
        </fieldset>
        <div className="min-w-0 px-1">
          <SmoothModeSetting />
        </div>
        <div className="min-w-0 px-1">
          <AppearanceDisplay />
        </div>
      </div>
    </div>
  )
}
