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
  lightweightStart: boolean
  restoreLastEntryOnStartup: boolean
  deviceName: string
}

function deriveStartupForm(general: GeneralSettings | undefined): StartupForm {
  return {
    autoStart: general?.autoStart ?? false,
    silentStart: general?.silentStart ?? false,
    lightweightStart: general?.lightweightStart ?? false,
    restoreLastEntryOnStartup: general?.restoreLastEntryOnStartup ?? false,
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
      // `lightweightStart` only takes effect on an auto-start launch; turning
      // autostart off would otherwise leave it checked but unreachable to
      // uncheck (its row disables when `!autoStart`). Clear it alongside so
      // there's no stale, stuck-on preference.
      if (!checked && form.lightweightStart) {
        await updateGeneralSetting({ lightweightStart: false })
      }
      setForm(prev => ({
        ...prev,
        autoStart: checked,
        lightweightStart: checked ? prev.lightweightStart : false,
      }))
    })

  const handleSilentStartChange = (checked: boolean) =>
    runSave('Failed to change silent-start setting', async () => {
      await updateGeneralSetting({ silentStart: checked })
      setForm(prev => ({ ...prev, silentStart: checked }))
    })

  const handleLightweightStartChange = (checked: boolean) =>
    runSave('Failed to change lightweight-start setting', async () => {
      await updateGeneralSetting({ lightweightStart: checked })
      setForm(prev => ({ ...prev, lightweightStart: checked }))
    })

  const handleRestoreLastEntryOnStartupChange = (checked: boolean) =>
    runSave('Failed to change restore-last-entry-on-startup setting', async () => {
      await updateGeneralSetting({ restoreLastEntryOnStartup: checked })
      setForm(prev => ({ ...prev, restoreLastEntryOnStartup: checked }))
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

      <SettingRow
        label={t('settings.sections.general.lightweightStart.label')}
        description={t('settings.sections.general.lightweightStart.description')}
      >
        <Switch
          checked={form.lightweightStart}
          onCheckedChange={handleLightweightStartChange}
          // Only takes effect on an OS auto-start launch — disabled until
          // "Launch at login" is on, so it can't be turned on to no effect.
          disabled={isBusy || !form.autoStart}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.restoreLastEntryOnStartup.label')}
        description={t('settings.sections.general.restoreLastEntryOnStartup.description')}
      >
        <Switch
          checked={form.restoreLastEntryOnStartup}
          onCheckedChange={handleRestoreLastEntryOnStartupChange}
          disabled={isBusy}
        />
      </SettingRow>
    </SettingGroup>
  )
}
