import { useTranslation } from 'react-i18next'
import { Switch } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'

const SecuritySection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, error, updateSecuritySetting } = useSetting()

  const [autoUnlockEnabled, setAutoUnlockEnabled] = useOptimisticSetting(
    setting?.security.autoUnlockEnabled ?? false,
    next => updateSecuritySetting({ autoUnlockEnabled: next }),
    { failureLog: 'Failed to change auto-unlock setting' }
  )

  // Display error message if there is an error
  if (error) {
    return (
      <div className="text-red-500 py-4">
        {t('settings.sections.security.loadError')}: {error}
      </div>
    )
  }

  return (
    <SettingGroup title={t('settings.sections.security.title')}>
      <SettingRow
        label={t('settings.sections.security.autoUnlock.label')}
        description={t('settings.sections.security.autoUnlock.description')}
      >
        <Switch checked={autoUnlockEnabled} onCheckedChange={setAutoUnlockEnabled} />
      </SettingRow>
    </SettingGroup>
  )
}

export default SecuritySection
