import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Switch } from '@/components/ui'
import { playUiSound, readStoredUiSoundEnabled, setUiSoundEnabled } from '@/lib/ui-sound'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'

export function SoundSettings() {
  const { t } = useTranslation()
  const [enabled, setEnabled] = useState(() => readStoredUiSoundEnabled())

  const handleChange = (checked: boolean) => {
    setEnabled(checked)
    setUiSoundEnabled(checked)
    if (checked) playUiSound('toggle')
  }

  return (
    <SettingGroup title={t('settings.sections.general.sound.title')}>
      <SettingRow
        label={t('settings.sections.general.sound.label')}
        description={t('settings.sections.general.sound.description')}
      >
        <Switch
          aria-label={t('settings.sections.general.sound.label')}
          checked={enabled}
          onCheckedChange={handleChange}
        />
      </SettingRow>
    </SettingGroup>
  )
}
