import { useTranslation } from 'react-i18next'
import { Switch } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useOptimisticSetting } from '../useOptimisticSetting'

export function TelemetrySettings() {
  const { t } = useTranslation()
  const { setting, updateGeneralSetting } = useSetting()

  const [telemetryEnabled, setTelemetryEnabled] = useOptimisticSetting(
    setting?.general.telemetryEnabled ?? true,
    next => updateGeneralSetting({ telemetryEnabled: next }),
    { failureLog: 'Failed to change diagnostics setting' }
  )
  const [usageAnalyticsEnabled, setUsageAnalyticsEnabled] = useOptimisticSetting(
    setting?.general.usageAnalyticsEnabled ?? true,
    next => updateGeneralSetting({ usageAnalyticsEnabled: next }),
    { failureLog: 'Failed to change usage analytics setting' }
  )

  return (
    <SettingGroup title={t('settings.sections.general.telemetry.title')}>
      <SettingRow
        label={t('settings.sections.general.telemetry.diagnostics.label')}
        description={t('settings.sections.general.telemetry.diagnostics.description')}
      >
        <Switch checked={telemetryEnabled} onCheckedChange={setTelemetryEnabled} />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.telemetry.usageAnalytics.label')}
        description={t('settings.sections.general.telemetry.usageAnalytics.description')}
      >
        <Switch checked={usageAnalyticsEnabled} onCheckedChange={setUsageAnalyticsEnabled} />
      </SettingRow>
    </SettingGroup>
  )
}
