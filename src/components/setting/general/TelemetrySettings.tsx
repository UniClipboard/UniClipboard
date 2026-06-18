import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Switch } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import type { GeneralSettings } from '@/types/setting'
import { SettingGroup } from '../SettingGroup'
import { SettingRow } from '../SettingRow'
import { useSavingState } from './useSavingState'

interface TelemetryForm {
  telemetryEnabled: boolean
  usageAnalyticsEnabled: boolean
}

function deriveTelemetryForm(general: GeneralSettings | undefined): TelemetryForm {
  return {
    telemetryEnabled: general?.telemetryEnabled ?? true,
    usageAnalyticsEnabled: general?.usageAnalyticsEnabled ?? true,
  }
}

export function TelemetrySettings() {
  const { t } = useTranslation()
  const { setting, loading, updateGeneralSetting } = useSetting()
  const { saving, runSave } = useSavingState()
  const [form, setForm] = useState(() => deriveTelemetryForm(setting?.general))
  const isBusy = loading || saving

  useEffect(() => {
    if (!setting?.general) return
    setForm(deriveTelemetryForm(setting.general))
  }, [setting])

  const handleTelemetryChange = (checked: boolean) =>
    runSave('Failed to change diagnostics setting', async () => {
      await updateGeneralSetting({ telemetryEnabled: checked })
      setForm(prev => ({ ...prev, telemetryEnabled: checked }))
    })

  const handleUsageAnalyticsChange = (checked: boolean) =>
    runSave('Failed to change usage analytics setting', async () => {
      await updateGeneralSetting({ usageAnalyticsEnabled: checked })
      setForm(prev => ({ ...prev, usageAnalyticsEnabled: checked }))
    })

  return (
    <SettingGroup title={t('settings.sections.general.telemetry.title')}>
      <SettingRow
        label={t('settings.sections.general.telemetry.diagnostics.label')}
        description={t('settings.sections.general.telemetry.diagnostics.description')}
      >
        <Switch
          checked={form.telemetryEnabled}
          onCheckedChange={handleTelemetryChange}
          disabled={isBusy}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.general.telemetry.usageAnalytics.label')}
        description={t('settings.sections.general.telemetry.usageAnalytics.description')}
      >
        <Switch
          checked={form.usageAnalyticsEnabled}
          onCheckedChange={handleUsageAnalyticsChange}
          disabled={isBusy}
        />
      </SettingRow>
    </SettingGroup>
  )
}
