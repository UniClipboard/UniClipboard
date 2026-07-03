import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Switch,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import type { GeneralSettings, StartupMode } from '@/types/setting'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useSavingState } from './useSavingState'

const STARTUP_MODES: StartupMode[] = ['normal', 'silent', 'lightweight']

interface StartupForm {
  autoStart: boolean
  startupMode: StartupMode
  restoreLastEntryOnStartup: boolean
  deviceName: string
}

function deriveStartupForm(general: GeneralSettings | undefined): StartupForm {
  return {
    autoStart: general?.autoStart ?? false,
    startupMode: general?.startupMode ?? 'normal',
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
    // launch registration. `startupMode` is an independent preference —
    // Lightweight mode only takes effect on an auto-start launch, but it
    // stays configurable either way; turning autostart off no longer resets it.
    runSave('Failed to change autostart setting', async () => {
      await updateAutostart(checked)
      setForm(prev => ({ ...prev, autoStart: checked }))
    })

  const handleStartupModeChange = (next: StartupMode) =>
    runSave('Failed to change startup-mode setting', async () => {
      await updateGeneralSetting({ startupMode: next })
      setForm(prev => ({ ...prev, startupMode: next }))
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
        label={t('settings.sections.general.startupMode.label')}
        description={t('settings.sections.general.startupMode.description')}
      >
        <div className="w-40">
          <Select
            value={form.startupMode}
            onValueChange={handleStartupModeChange}
            disabled={isBusy}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {STARTUP_MODES.map(mode => (
                <SelectItem key={mode} value={mode}>
                  {t(`settings.sections.general.startupMode.options.${mode}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
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
