import { useTranslation } from 'react-i18next'
import { Input } from '@/components/motion/input'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { SettingRow } from '@/components/setting/SettingRow'
import { useOptimisticSetting } from '@/components/setting/useOptimisticSetting'
import { Switch } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { AutoSaveDirectoryRow } from './AutoSaveDirectoryRow'
import { useFileSyncNumbers } from './useFileSyncNumbers'

export function FileSyncSettingsGroup({ syncEnabled }: { syncEnabled: boolean }) {
  const { t } = useTranslation()
  const { setting, updateFileSyncSetting } = useSetting()
  const [fileSyncEnabled, setFileSyncEnabled] = useOptimisticSetting(
    setting?.fileSync?.fileSyncEnabled ?? true,
    next => updateFileSyncSetting({ fileSyncEnabled: next }),
    { failureLog: 'Failed to change file-sync setting' }
  )
  const [fileAutoCleanup, setFileAutoCleanup] = useOptimisticSetting(
    setting?.fileSync?.fileAutoCleanup ?? true,
    next => updateFileSyncSetting({ fileAutoCleanup: next }),
    { failureLog: 'Failed to change file auto-cleanup setting' }
  )
  const { fields, change } = useFileSyncNumbers(setting?.fileSync, updateFileSyncSetting)

  const fileControlsDisabled = !syncEnabled || !fileSyncEnabled

  return (
    <SettingGroup title={t('settings.sections.sync.fileSync.title')}>
      {/* Enable file sync toggle */}
      <SettingRow
        label={t('settings.sections.sync.fileSync.enable.label')}
        description={t('settings.sections.sync.fileSync.enable.description')}
      >
        <Switch
          id="file-sync-enabled"
          aria-label={t('settings.sections.sync.fileSync.enable.label')}
          checked={fileSyncEnabled}
          onCheckedChange={setFileSyncEnabled}
          disabled={!syncEnabled}
        />
      </SettingRow>

      <AutoSaveDirectoryRow disabled={fileControlsDisabled} />

      {/* Small file threshold */}
      <SettingRow
        label={t('settings.sections.sync.fileSync.smallFileThreshold.label')}
        description={t('settings.sections.sync.fileSync.smallFileThreshold.description')}
      >
        <Input
          aria-label={t('settings.sections.sync.fileSync.smallFileThreshold.label')}
          inputMode="numeric"
          value={fields.smallFileThreshold.value}
          onChange={value => change('smallFileThreshold', value)}
          error={fields.smallFileThreshold.error ? t(fields.smallFileThreshold.error) : undefined}
          className="w-44 max-w-full"
          classNames={{
            field: 'h-9 rounded-lg bg-card',
            input: 'text-right tabular-nums',
            rightIcon: 'pr-3 text-ui-caption',
          }}
          rightIcon={t('settings.sections.sync.fileSync.smallFileThreshold.unit')}
          disabled={fileControlsDisabled}
        />
      </SettingRow>

      {/* Max file size limit */}
      <SettingRow
        label={t('settings.sections.sync.fileSync.maxFileSize.label')}
        description={t('settings.sections.sync.fileSync.maxFileSize.description')}
      >
        <Input
          aria-label={t('settings.sections.sync.fileSync.maxFileSize.label')}
          inputMode="numeric"
          value={fields.maxFileSize.value}
          onChange={value => change('maxFileSize', value)}
          error={fields.maxFileSize.error ? t(fields.maxFileSize.error) : undefined}
          className="w-44 max-w-full"
          classNames={{
            field: 'h-9 rounded-lg bg-card',
            input: 'text-right tabular-nums',
            rightIcon: 'pr-3 text-ui-caption',
          }}
          rightIcon={t('settings.sections.sync.fileSync.maxFileSize.unit')}
          disabled={fileControlsDisabled}
        />
      </SettingRow>

      {/* Per-device cache quota */}
      <SettingRow
        label={t('settings.sections.sync.fileSync.cacheQuota.label')}
        description={t('settings.sections.sync.fileSync.cacheQuota.description')}
      >
        <Input
          aria-label={t('settings.sections.sync.fileSync.cacheQuota.label')}
          inputMode="numeric"
          value={fields.fileCacheQuotaPerDevice.value}
          onChange={value => change('fileCacheQuotaPerDevice', value)}
          error={
            fields.fileCacheQuotaPerDevice.error
              ? t(fields.fileCacheQuotaPerDevice.error)
              : undefined
          }
          className="w-44 max-w-full"
          classNames={{
            field: 'h-9 rounded-lg bg-card',
            input: 'text-right tabular-nums',
            rightIcon: 'pr-3 text-ui-caption',
          }}
          rightIcon={t('settings.sections.sync.fileSync.cacheQuota.unit')}
          disabled={fileControlsDisabled}
        />
      </SettingRow>

      {/* File retention period */}
      <SettingRow
        label={t('settings.sections.sync.fileSync.retentionPeriod.label')}
        description={t('settings.sections.sync.fileSync.retentionPeriod.description')}
      >
        <Input
          aria-label={t('settings.sections.sync.fileSync.retentionPeriod.label')}
          inputMode="numeric"
          value={fields.fileRetentionHours.value}
          onChange={value => change('fileRetentionHours', value)}
          error={fields.fileRetentionHours.error ? t(fields.fileRetentionHours.error) : undefined}
          className="w-44 max-w-full"
          classNames={{
            field: 'h-9 rounded-lg bg-card',
            input: 'text-right tabular-nums',
            rightIcon: 'pr-3 text-ui-caption',
          }}
          rightIcon={t('settings.sections.sync.fileSync.retentionPeriod.unit')}
          disabled={fileControlsDisabled}
        />
      </SettingRow>

      {/* Auto-cleanup toggle */}
      <SettingRow
        label={t('settings.sections.sync.fileSync.autoCleanup.label')}
        description={t('settings.sections.sync.fileSync.autoCleanup.description')}
      >
        <Switch
          id="file-auto-cleanup"
          aria-label={t('settings.sections.sync.fileSync.autoCleanup.label')}
          checked={fileAutoCleanup}
          onCheckedChange={setFileAutoCleanup}
          disabled={fileControlsDisabled}
        />
      </SettingRow>
    </SettingGroup>
  )
}
