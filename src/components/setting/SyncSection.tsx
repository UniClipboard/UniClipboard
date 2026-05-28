import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Switch, Input, Badge } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'

const MB = 1024 * 1024

/** Convert bytes to MB (integer) */
const bytesToMb = (bytes: number): number => Math.round(bytes / MB)

/** Convert MB to bytes */
const mbToBytes = (mb: number): number => mb * MB

const SyncSection: React.FC = () => {
  const { t } = useTranslation()
  // Use setting context
  const { setting, error, updateSyncSetting, updateFileSyncSetting } = useSetting()

  // Local state for UI display - initialize from setting to avoid flash
  const [autoSync, setAutoSync] = useState(setting?.sync.autoSync ?? true)

  // File sync local state
  const [fileSyncEnabled, setFileSyncEnabled] = useState(setting?.fileSync?.fileSyncEnabled ?? true)
  const [smallFileThreshold, setSmallFileThreshold] = useState(() =>
    bytesToMb(setting?.fileSync?.smallFileThreshold ?? 10 * MB)
  )
  const [smallFileThresholdError, setSmallFileThresholdError] = useState<string | null>(null)
  const [maxFileSizeLimit, setMaxFileSizeLimit] = useState(() =>
    bytesToMb(setting?.fileSync?.maxFileSize ?? 5120 * MB)
  )
  const [maxFileSizeLimitError, setMaxFileSizeLimitError] = useState<string | null>(null)
  const [cacheQuota, setCacheQuota] = useState(() =>
    bytesToMb(setting?.fileSync?.fileCacheQuotaPerDevice ?? 500 * MB)
  )
  const [cacheQuotaError, setCacheQuotaError] = useState<string | null>(null)
  const [retentionHours, setRetentionHours] = useState(setting?.fileSync?.fileRetentionHours ?? 24)
  const [retentionHoursError, setRetentionHoursError] = useState<string | null>(null)
  const [fileAutoCleanup, setFileAutoCleanup] = useState(setting?.fileSync?.fileAutoCleanup ?? true)

  // Sync frequency options (for display in coming-soon label)
  const syncFrequencyOptions = [
    { value: 'realtime', label: t('settings.sections.sync.syncFrequency.realtime') },
    { value: '30s', label: t('settings.sections.sync.syncFrequency.30s') },
    { value: '1m', label: t('settings.sections.sync.syncFrequency.1m') },
    { value: '5m', label: t('settings.sections.sync.syncFrequency.5m') },
    { value: '15m', label: t('settings.sections.sync.syncFrequency.15m') },
  ]

  // Update local state when settings are loaded
  useEffect(() => {
    if (setting) {
      setAutoSync(setting.sync.autoSync)

      // File sync settings
      setFileSyncEnabled(setting.fileSync?.fileSyncEnabled ?? true)
      setSmallFileThreshold(bytesToMb(setting.fileSync?.smallFileThreshold ?? 10 * MB))
      setMaxFileSizeLimit(bytesToMb(setting.fileSync?.maxFileSize ?? 5120 * MB))
      setCacheQuota(bytesToMb(setting.fileSync?.fileCacheQuotaPerDevice ?? 500 * MB))
      setRetentionHours(setting.fileSync?.fileRetentionHours ?? 24)
      setFileAutoCleanup(setting.fileSync?.fileAutoCleanup ?? true)
    }
  }, [setting])

  // Handle auto sync switch change
  const handleAutoSyncChange = (checked: boolean) => {
    setAutoSync(checked)
    updateSyncSetting({ autoSync: checked })
  }

  // --- File sync handlers ---

  const handleFileSyncEnabledChange = (checked: boolean) => {
    setFileSyncEnabled(checked)
    updateFileSyncSetting({ fileSyncEnabled: checked })
  }

  const handleSmallFileThresholdChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (!value.trim()) {
      setSmallFileThresholdError(null)
      setSmallFileThreshold(0)
      return
    }

    if (!/^\d+$/.test(value)) {
      setSmallFileThresholdError(
        t('settings.sections.sync.fileSync.smallFileThreshold.errors.invalid')
      )
      setSmallFileThreshold(parseInt(value) || 0)
      return
    }

    const size = parseInt(value)
    setSmallFileThreshold(size)

    if (size < 1 || size > 1000) {
      setSmallFileThresholdError(
        t('settings.sections.sync.fileSync.smallFileThreshold.errors.range')
      )
      return
    }

    if (size >= maxFileSizeLimit) {
      setSmallFileThresholdError(
        t('settings.sections.sync.fileSync.smallFileThreshold.errors.exceedsMax')
      )
      return
    }

    setSmallFileThresholdError(null)
    updateFileSyncSetting({ smallFileThreshold: mbToBytes(size) })
  }

  const handleMaxFileSizeLimitChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (!value.trim()) {
      setMaxFileSizeLimitError(null)
      setMaxFileSizeLimit(0)
      return
    }

    if (!/^\d+$/.test(value)) {
      setMaxFileSizeLimitError(t('settings.sections.sync.fileSync.maxFileSize.errors.invalid'))
      setMaxFileSizeLimit(parseInt(value) || 0)
      return
    }

    const size = parseInt(value)
    setMaxFileSizeLimit(size)

    if (size < 1 || size > 10240) {
      setMaxFileSizeLimitError(t('settings.sections.sync.fileSync.maxFileSize.errors.range'))
      return
    }

    setMaxFileSizeLimitError(null)
    updateFileSyncSetting({ maxFileSize: mbToBytes(size) })
  }

  const handleCacheQuotaChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (!value.trim()) {
      setCacheQuotaError(null)
      setCacheQuota(0)
      return
    }

    if (!/^\d+$/.test(value)) {
      setCacheQuotaError(t('settings.sections.sync.fileSync.cacheQuota.errors.invalid'))
      setCacheQuota(parseInt(value) || 0)
      return
    }

    const size = parseInt(value)
    setCacheQuota(size)

    if (size < 50 || size > 10240) {
      setCacheQuotaError(t('settings.sections.sync.fileSync.cacheQuota.errors.range'))
      return
    }

    setCacheQuotaError(null)
    updateFileSyncSetting({ fileCacheQuotaPerDevice: mbToBytes(size) })
  }

  const handleRetentionHoursChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (!value.trim()) {
      setRetentionHoursError(null)
      setRetentionHours(0)
      return
    }

    if (!/^\d+$/.test(value)) {
      setRetentionHoursError(t('settings.sections.sync.fileSync.retentionPeriod.errors.invalid'))
      setRetentionHours(parseInt(value) || 0)
      return
    }

    const hours = parseInt(value)
    setRetentionHours(hours)

    if (hours < 1 || hours > 720) {
      setRetentionHoursError(t('settings.sections.sync.fileSync.retentionPeriod.errors.range'))
      return
    }

    setRetentionHoursError(null)
    updateFileSyncSetting({ fileRetentionHours: hours })
  }

  const handleFileAutoCleanupChange = (checked: boolean) => {
    setFileAutoCleanup(checked)
    updateFileSyncSetting({ fileAutoCleanup: checked })
  }

  // Show error message if any
  if (error) {
    return (
      <div className="text-destructive py-4">
        {t('settings.sections.sync.loadError')} {error}
      </div>
    )
  }

  return (
    <>
      <SettingGroup title={t('settings.categories.sync')}>
        <SettingRow
          label={t('settings.sections.sync.autoSync.label')}
          description={t('settings.sections.sync.autoSync.description')}
        >
          <Switch id="auto-sync" checked={autoSync} onCheckedChange={handleAutoSyncChange} />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.sync.syncFrequency.label')}
          description={t('settings.sections.sync.syncFrequency.description')}
        >
          <div className="flex items-center gap-2">
            <span className="text-sm text-muted-foreground">
              {syncFrequencyOptions.find(
                o => o.value === (setting?.sync.syncFrequency ?? 'realtime')
              )?.label ?? t('settings.sections.sync.syncFrequency.realtime')}
            </span>
            <Badge variant="secondary">{t('devices.settings.badges.comingSoon')}</Badge>
          </div>
        </SettingRow>
      </SettingGroup>

      <div className="mt-6">
        <SettingGroup title={t('settings.sections.sync.fileSync.title')}>
          {/* Enable file sync toggle */}
          <SettingRow
            label={t('settings.sections.sync.fileSync.enable.label')}
            description={t('settings.sections.sync.fileSync.enable.description')}
          >
            <Switch
              id="file-sync-enabled"
              checked={fileSyncEnabled}
              onCheckedChange={handleFileSyncEnabledChange}
              disabled={!autoSync}
            />
          </SettingRow>

          {/* Small file threshold */}
          <SettingRow
            label={t('settings.sections.sync.fileSync.smallFileThreshold.label')}
            description={t('settings.sections.sync.fileSync.smallFileThreshold.description')}
          >
            <div className="flex flex-col items-end gap-1">
              <div className="flex items-center gap-2">
                <Input
                  type="text"
                  value={smallFileThreshold.toString()}
                  onChange={handleSmallFileThresholdChange}
                  className={smallFileThresholdError ? 'border-red-500 w-32' : 'w-32'}
                  disabled={!autoSync || !fileSyncEnabled}
                />
                <span className="text-sm text-muted-foreground">
                  {t('settings.sections.sync.fileSync.smallFileThreshold.unit')}
                </span>
              </div>
              {smallFileThresholdError && (
                <p className="text-xs text-red-500">{smallFileThresholdError}</p>
              )}
            </div>
          </SettingRow>

          {/* Max file size limit */}
          <SettingRow
            label={t('settings.sections.sync.fileSync.maxFileSize.label')}
            description={t('settings.sections.sync.fileSync.maxFileSize.description')}
          >
            <div className="flex flex-col items-end gap-1">
              <div className="flex items-center gap-2">
                <Input
                  type="text"
                  value={maxFileSizeLimit.toString()}
                  onChange={handleMaxFileSizeLimitChange}
                  className={maxFileSizeLimitError ? 'border-red-500 w-32' : 'w-32'}
                  disabled={!autoSync || !fileSyncEnabled}
                />
                <span className="text-sm text-muted-foreground">
                  {t('settings.sections.sync.fileSync.maxFileSize.unit')}
                </span>
              </div>
              {maxFileSizeLimitError && (
                <p className="text-xs text-red-500">{maxFileSizeLimitError}</p>
              )}
            </div>
          </SettingRow>

          {/* Per-device cache quota */}
          <SettingRow
            label={t('settings.sections.sync.fileSync.cacheQuota.label')}
            description={t('settings.sections.sync.fileSync.cacheQuota.description')}
          >
            <div className="flex flex-col items-end gap-1">
              <div className="flex items-center gap-2">
                <Input
                  type="text"
                  value={cacheQuota.toString()}
                  onChange={handleCacheQuotaChange}
                  className={cacheQuotaError ? 'border-red-500 w-32' : 'w-32'}
                  disabled={!autoSync || !fileSyncEnabled}
                />
                <span className="text-sm text-muted-foreground">
                  {t('settings.sections.sync.fileSync.cacheQuota.unit')}
                </span>
              </div>
              {cacheQuotaError && <p className="text-xs text-red-500">{cacheQuotaError}</p>}
            </div>
          </SettingRow>

          {/* File retention period */}
          <SettingRow
            label={t('settings.sections.sync.fileSync.retentionPeriod.label')}
            description={t('settings.sections.sync.fileSync.retentionPeriod.description')}
          >
            <div className="flex flex-col items-end gap-1">
              <div className="flex items-center gap-2">
                <Input
                  type="text"
                  value={retentionHours.toString()}
                  onChange={handleRetentionHoursChange}
                  className={retentionHoursError ? 'border-red-500 w-32' : 'w-32'}
                  disabled={!autoSync || !fileSyncEnabled}
                />
                <span className="text-sm text-muted-foreground">
                  {t('settings.sections.sync.fileSync.retentionPeriod.unit')}
                </span>
              </div>
              {retentionHoursError && <p className="text-xs text-red-500">{retentionHoursError}</p>}
            </div>
          </SettingRow>

          {/* Auto-cleanup toggle */}
          <SettingRow
            label={t('settings.sections.sync.fileSync.autoCleanup.label')}
            description={t('settings.sections.sync.fileSync.autoCleanup.description')}
          >
            <Switch
              id="file-auto-cleanup"
              checked={fileAutoCleanup}
              onCheckedChange={handleFileAutoCleanupChange}
              disabled={!autoSync || !fileSyncEnabled}
            />
          </SettingRow>
        </SettingGroup>
      </div>
    </>
  )
}

export default SyncSection
