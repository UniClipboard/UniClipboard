import { useTranslation } from 'react-i18next'
import {
  Switch,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import type { StartupMode } from '@/types/setting'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useOptimisticSetting } from '../useOptimisticSetting'

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

  return (
    <SettingGroup title={t('settings.sections.general.startupTitle')}>
      <SettingRow
        label={t('settings.sections.general.autoStart.label')}
        description={t('settings.sections.general.autoStart.description')}
      >
        <Switch
          aria-label={t('settings.sections.general.autoStart.label')}
          checked={autoStart}
          onCheckedChange={setAutoStart}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.startupMode.label')}
        description={t(`settings.sections.general.startupMode.summaries.${startupMode}`)}
      >
        <div className="w-40">
          <Select value={startupMode} onValueChange={next => setStartupMode(next as StartupMode)}>
            <SelectTrigger
              aria-label={t('settings.sections.general.startupMode.label')}
              className="w-full"
            >
              <SelectValue>
                {t(`settings.sections.general.startupMode.options.${startupMode}`)}
              </SelectValue>
            </SelectTrigger>
            <SelectContent className="w-72 max-w-[calc(100vw-2rem)]">
              {STARTUP_MODES.map(mode => (
                <SelectItem
                  key={mode}
                  value={mode}
                  aria-label={t(`settings.sections.general.startupMode.options.${mode}`)}
                >
                  <span className="flex min-w-0 flex-col gap-1">
                    <span>{t(`settings.sections.general.startupMode.options.${mode}`)}</span>
                    <span className="text-xs leading-relaxed text-muted-foreground">
                      {t(`settings.sections.general.startupMode.summaries.${mode}`)}
                    </span>
                  </span>
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
          aria-label={t('settings.sections.general.restoreLastEntryOnStartup.label')}
          checked={restoreLastEntry}
          onCheckedChange={setRestoreLastEntry}
        />
      </SettingRow>
    </SettingGroup>
  )
}
