import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { Switch } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'

const SecuritySection: React.FC = () => {
  const { t } = useTranslation()
  const { setting, error, updateSecuritySetting } = useSetting()

  const [autoUnlockEnabled, setAutoUnlockEnabled] = useState(
    setting?.security.auto_unlock_enabled ?? false
  )

  // Update local state when settings are loaded
  useEffect(() => {
    if (setting) {
      setAutoUnlockEnabled(setting.security.auto_unlock_enabled)
    }
  }, [setting])

  const handleAutoUnlockChange = (checked: boolean) => {
    setAutoUnlockEnabled(checked)
    updateSecuritySetting({ auto_unlock_enabled: checked })
  }

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
        <Switch checked={autoUnlockEnabled} onCheckedChange={handleAutoUnlockChange} />
      </SettingRow>
    </SettingGroup>
  )
}

export default SecuritySection
