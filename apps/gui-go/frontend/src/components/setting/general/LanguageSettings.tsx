import { useTranslation } from 'react-i18next'
import { useSetting } from '@/hooks/useSetting'
import { SUPPORTED_LANGUAGES, type SupportedLanguage, getInitialLanguage } from '@/i18n'
import { SettingSelect } from '../SettingControls'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useOptimisticSetting } from '../useOptimisticSetting'

/** Validate a backend language value, falling back to the detected default. */
function resolveLanguage(raw: string | null | undefined): SupportedLanguage {
  const isValid = raw && SUPPORTED_LANGUAGES.includes(raw as SupportedLanguage)
  return isValid ? (raw as SupportedLanguage) : getInitialLanguage()
}

export function LanguageSettings() {
  const { t } = useTranslation()
  const { setting, updateGeneralSetting } = useSetting()

  const [language, setLanguage] = useOptimisticSetting<SupportedLanguage>(
    resolveLanguage(setting?.general.language),
    next => updateGeneralSetting({ language: next }),
    { failureLog: 'Failed to change language setting' }
  )

  return (
    <SettingGroup title={t('settings.sections.general.language.title')}>
      <SettingRow
        label={t('settings.sections.general.language.label')}
        description={t('settings.sections.general.language.description')}
      >
        <SettingSelect
          label={t('settings.sections.general.language.label')}
          value={language}
          onValueChange={next => setLanguage((next as SupportedLanguage) || getInitialLanguage())}
          items={SUPPORTED_LANGUAGES.map(lang => ({ value: lang, label: t(`language.${lang}`) }))}
        />
      </SettingRow>
    </SettingGroup>
  )
}
