import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Switch, Input } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import type { GeneralSettings } from '@/types/setting'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useSavingState } from './useSavingState'

interface StartupForm {
  autoStart: boolean
  silentStart: boolean
  deviceName: string
}

function deriveStartupForm(general: GeneralSettings | undefined): StartupForm {
  return {
    autoStart: general?.autoStart ?? false,
    silentStart: general?.silentStart ?? false,
    deviceName: general?.deviceName ?? '',
  }
}

export function StartupSettings() {
  const { t } = useTranslation()
  const { setting, loading, updateAutostart, updateGeneralSetting } = useSetting()
  const { saving, runSave } = useSavingState()
  // Mirror the persisted fields in a single object so re-hydration is one
  // setState, not a cascade of individual setters.
  const [form, setForm] = useState(() => deriveStartupForm(setting?.general))
  const isBusy = loading || saving

  useEffect(() => {
    if (!setting?.general) return
    setForm(deriveStartupForm(setting.general))
  }, [setting])

  const handleAutoStartChange = (checked: boolean) =>
    // Autostart still uses the dedicated host command because it applies OS
    // launch registration.
    runSave('Failed to change autostart setting', async () => {
      await updateAutostart(checked)
      setForm(prev => ({ ...prev, autoStart: checked }))
    })

  const handleSilentStartChange = (checked: boolean) =>
    runSave('Failed to change silent-start setting', async () => {
      await updateGeneralSetting({ silentStart: checked })
      setForm(prev => ({ ...prev, silentStart: checked }))
    })

  const handleDeviceNameChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setForm(prev => ({ ...prev, deviceName: e.target.value }))

  const handleDeviceNameBlur = () =>
    runSave('Failed to change device name', async () => {
      await updateGeneralSetting({ deviceName: form.deviceName })
    })

  return (
    <SettingGroup title={t('settings.sections.general.startupTitle')}>
      <SettingRow
        label={t('settings.sections.general.deviceName.label')}
        description={t('settings.sections.general.deviceName.description')}
      >
        <div className="w-40">
          <Input
            value={form.deviceName}
            onChange={handleDeviceNameChange}
            onBlur={handleDeviceNameBlur}
            placeholder={t('settings.sections.general.deviceName.placeholder')}
            disabled={isBusy}
          />
        </div>
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.autoStart.label')}
        description={t('settings.sections.general.autoStart.description')}
      >
        <Switch
          checked={form.autoStart}
          onCheckedChange={handleAutoStartChange}
          disabled={isBusy}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.silentStart.label')}
        description={t('settings.sections.general.silentStart.description')}
      >
        <Switch
          checked={form.silentStart}
          onCheckedChange={handleSilentStartChange}
          disabled={isBusy}
        />
      </SettingRow>
    </SettingGroup>
  )
}
