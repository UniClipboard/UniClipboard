import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { SUPPORTED_LANGUAGES, type SupportedLanguage, getInitialLanguage } from '@/i18n'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useSavingState } from './useSavingState'

/** Validate a backend language value, falling back to the detected default. */
function resolveLanguage(raw: string | null | undefined): SupportedLanguage {
  const isValid = raw && SUPPORTED_LANGUAGES.includes(raw as SupportedLanguage)
  return isValid ? (raw as SupportedLanguage) : getInitialLanguage()
}

export function LanguageSettings() {
  const { t } = useTranslation()
  const { setting, loading, updateGeneralSetting } = useSetting()
  const { saving, runSave } = useSavingState()
  const [language, setLanguage] = useState<SupportedLanguage>(() =>
    resolveLanguage(setting?.general.language)
  )
  const isBusy = loading || saving

  useEffect(() => {
    if (!setting?.general) return
    setLanguage(resolveLanguage(setting.general.language))
  }, [setting])

  const handleLanguageChange = (next: string) =>
    runSave('Failed to change language setting', async () => {
      const normalized = (next as SupportedLanguage) || getInitialLanguage()
      await updateGeneralSetting({ language: normalized })
      setLanguage(normalized)
    })

  return (
    <SettingGroup title={t('settings.sections.general.language.title')}>
      <SettingRow
        label={t('settings.sections.general.language.label')}
        description={t('settings.sections.general.language.description')}
      >
        <div className="w-40">
          <Select value={language} onValueChange={handleLanguageChange} disabled={isBusy}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SUPPORTED_LANGUAGES.map(lang => (
                <SelectItem key={lang} value={lang}>
                  {t(`language.${lang}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </SettingRow>
    </SettingGroup>
  )
}
