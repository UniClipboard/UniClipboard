import { useTranslation } from 'react-i18next'
import { useSetting } from '@/hooks/useSetting'
import type { StartupMode } from '@/types/setting'
import { SettingSelect, SettingSwitch } from '../SettingControls'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useOptimisticSetting } from '../useOptimisticSetting'

const STARTUP_MODES: StartupMode[] = ['normal', 'lightweight']

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
    setting?.general.startupMode === 'silent'
      ? 'normal'
      : (setting?.general.startupMode ?? 'normal'),
    next => updateGeneralSetting({ startupMode: next }),
    { failureLog: 'Failed to change startup-mode setting' }
  )
  const [backgroundStartup, setBackgroundStartup] = useOptimisticSetting(
    setting?.general.backgroundStartup ??
      (setting?.general.startupMode === 'silent' || setting?.general.startupMode === 'lightweight'),
    next => updateGeneralSetting({ backgroundStartup: next }),
    { failureLog: 'Failed to change background startup setting' }
  )
  const [restoreLastEntry, setRestoreLastEntry] = useOptimisticSetting(
    setting?.general.restoreLastEntryOnStartup ?? false,
    next => updateGeneralSetting({ restoreLastEntryOnStartup: next }),
    { failureLog: 'Failed to change restore-last-entry-on-startup setting' }
  )

  return (
    <SettingGroup title={t('settings.sections.general.startupTitle')}>
      <SettingRow
        label={t('settings.sections.general.autoStart.label')}
        description={t('settings.sections.general.autoStart.description')}
      >
        <SettingSwitch
          label={t('settings.sections.general.autoStart.label')}
          checked={autoStart}
          onCheckedChange={setAutoStart}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.startupMode.label')}
        description={t(`settings.sections.general.startupMode.summaries.${startupMode}`)}
      >
        <SettingSelect
          label={t('settings.sections.general.startupMode.label')}
          value={startupMode}
          onValueChange={next => setStartupMode(next as StartupMode)}
          items={STARTUP_MODES.map(mode => ({
            value: mode,
            label: t(`settings.sections.general.startupMode.options.${mode}`),
            description: t(`settings.sections.general.startupMode.summaries.${mode}`),
          }))}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.backgroundStartup.label')}
        description={t('settings.sections.general.backgroundStartup.description')}
      >
        <SettingSwitch
          label={t('settings.sections.general.backgroundStartup.label')}
          checked={backgroundStartup}
          onCheckedChange={setBackgroundStartup}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.restoreLastEntryOnStartup.label')}
        description={t('settings.sections.general.restoreLastEntryOnStartup.description')}
      >
        <SettingSwitch
          label={t('settings.sections.general.restoreLastEntryOnStartup.label')}
          checked={restoreLastEntry}
          onCheckedChange={setRestoreLastEntry}
        />
      </SettingRow>
    </SettingGroup>
  )
}
