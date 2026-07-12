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
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import type { StartupMode } from '@/types/setting'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useOptimisticSetting } from '../useOptimisticSetting'

const log = createLogger('startup-settings')

const STARTUP_MODES: StartupMode[] = ['normal', 'silent', 'lightweight']

export function StartupSettings() {
  const { t } = useTranslation()
  const { setting, updateAutostart, updateGeneralSetting } = useSetting()

  // Autostart still routes through the dedicated host command (it applies OS
  // launch registration), the others through the daemon settings API — but all
  // are optimistic: they reflect immediately and persist in the background.
  const [autoStart, setAutoStart] = useOptimisticSetting(
    setting?.general.autoStart ?? false,
    next => updateAutostart(next),
    { failureLog: 'Failed to change autostart setting' }
  )
  const [startupMode, setStartupMode] = useOptimisticSetting<StartupMode>(
    setting?.general.startupMode ?? 'normal',
    next => updateGeneralSetting({ startupMode: next }),
    { failureLog: 'Failed to change startup-mode setting' }
  )
  const [restoreLastEntry, setRestoreLastEntry] = useOptimisticSetting(
    setting?.general.restoreLastEntryOnStartup ?? false,
    next => updateGeneralSetting({ restoreLastEntryOnStartup: next }),
    { failureLog: 'Failed to change restore-last-entry-on-startup setting' }
  )

  // Device name is free text edited continuously and persisted on blur, so it
  // keeps its own local edit state rather than the optimistic-toggle hook.
  const [deviceName, setDeviceName] = useState(setting?.general.deviceName ?? '')
  useEffect(() => {
    setDeviceName(setting?.general.deviceName ?? '')
  }, [setting?.general?.deviceName])

  const handleDeviceNameBlur = () => {
    if (deviceName === (setting?.general.deviceName ?? '')) return
    updateGeneralSetting({ deviceName }).catch((err: unknown) => {
      log.error({ err }, 'Failed to change device name')
      toast.error(t('settings.sections.general.saveError'))
    })
  }

  return (
    <SettingGroup title={t('settings.sections.general.startupTitle')}>
      <SettingRow
        label={t('settings.sections.general.deviceName.label')}
        description={t('settings.sections.general.deviceName.description')}
      >
        <div className="w-40">
          <Input
            value={deviceName}
            onChange={e => setDeviceName(e.target.value)}
            onBlur={handleDeviceNameBlur}
            placeholder={t('settings.sections.general.deviceName.placeholder')}
          />
        </div>
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.autoStart.label')}
        description={t('settings.sections.general.autoStart.description')}
      >
        <Switch checked={autoStart} onCheckedChange={setAutoStart} />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.startupMode.label')}
        description={t('settings.sections.general.startupMode.description')}
      >
        <div className="w-40">
          <Select value={startupMode} onValueChange={next => setStartupMode(next as StartupMode)}>
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
        <Switch checked={restoreLastEntry} onCheckedChange={setRestoreLastEntry} />
      </SettingRow>
    </SettingGroup>
  )
}
