import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Input } from '@/components/motion/input'
import { SettingGroup } from '@/components/setting/SettingGroup'
import { SettingRow } from '@/components/setting/SettingRow'
import { useOptimisticSetting } from '@/components/setting/useOptimisticSetting'
import { Switch, Badge, Button } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('sync-section')

const MB = 1024 * 1024

/** Convert bytes to MB (integer) */
const bytesToMb = (bytes: number): number => Math.round(bytes / MB)

/** Convert MB to bytes */
const mbToBytes = (mb: number): number => mb * MB

const SyncSection: React.FC = () => {
  const { t } = useTranslation()
  // Use setting context
  const { setting, error, updateSyncSetting, updateFileSyncSetting } = useSetting()

  const [syncEnabled, setSyncEnabled] = useOptimisticSetting(
    setting?.sync.syncEnabled ?? true,
    next => updateSyncSetting({ syncEnabled: next }),
    { failureLog: 'Failed to change sync setting' }
  )
  const [autoSyncEnabled, setAutoSyncEnabled] = useOptimisticSetting(
    setting?.sync.autoSyncEnabled ?? true,
    next => updateSyncSetting({ autoSyncEnabled: next }),
    { failureLog: 'Failed to change auto-sync setting' }
  )
  const [syncOnRestore, setSyncOnRestore] = useOptimisticSetting(
    setting?.sync.syncOnRestore ?? false,
    next => updateSyncSetting({ syncOnRestore: next }),
    { failureLog: 'Failed to change sync-on-restore setting' }
  )
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
  const persistedSmallFileThreshold = bytesToMb(setting?.fileSync?.smallFileThreshold ?? 10 * MB)
  const persistedMaxFileSizeLimit = bytesToMb(setting?.fileSync?.maxFileSize ?? 5120 * MB)
  const persistedCacheQuota = bytesToMb(setting?.fileSync?.fileCacheQuotaPerDevice ?? 500 * MB)
  const persistedRetentionHours = setting?.fileSync?.fileRetentionHours ?? 24
  const [smallFileThresholdDraft, setSmallFileThresholdDraft] = useState<string | null>(null)
  const [smallFileThresholdError, setSmallFileThresholdError] = useState<string | null>(null)
  const [maxFileSizeLimitDraft, setMaxFileSizeLimitDraft] = useState<string | null>(null)
  const [maxFileSizeLimitError, setMaxFileSizeLimitError] = useState<string | null>(null)
  const [cacheQuotaDraft, setCacheQuotaDraft] = useState<string | null>(null)
  const [cacheQuotaError, setCacheQuotaError] = useState<string | null>(null)
  const [retentionHoursDraft, setRetentionHoursDraft] = useState<string | null>(null)
  const [retentionHoursError, setRetentionHoursError] = useState<string | null>(null)
  const smallFileThresholdValue = smallFileThresholdDraft ?? String(persistedSmallFileThreshold)
  const maxFileSizeLimitValue = maxFileSizeLimitDraft ?? String(persistedMaxFileSizeLimit)
  const cacheQuotaValue = cacheQuotaDraft ?? String(persistedCacheQuota)
  const retentionHoursValue = retentionHoursDraft ?? String(persistedRetentionHours)
  const effectiveSmallFileThreshold = Number.parseInt(smallFileThresholdValue, 10) || 0
  const effectiveMaxFileSizeLimit = Number.parseInt(maxFileSizeLimitValue, 10) || 0

  // Auto-save directory for inbound files (null/undefined ⇒ managed storage).
  const autoSaveDir = setting?.fileSync?.autoSaveDir ?? null
  const [savingAutoSaveDir, setSavingAutoSaveDir] = useState(false)

  // Sync frequency options (for display in coming-soon label)
  const syncFrequencyOptions = [
    { value: 'realtime', label: t('settings.sections.sync.syncFrequency.realtime') },
    { value: '30s', label: t('settings.sections.sync.syncFrequency.30s') },
    { value: '1m', label: t('settings.sections.sync.syncFrequency.1m') },
    { value: '5m', label: t('settings.sections.sync.syncFrequency.5m') },
    { value: '15m', label: t('settings.sections.sync.syncFrequency.15m') },
  ]

  const handleSmallFileThresholdChange = (value: string) => {
    setSmallFileThresholdDraft(value)

    if (!value.trim()) {
      setSmallFileThresholdError(null)
      return
    }

    if (!/^\d+$/.test(value)) {
      setSmallFileThresholdError(
        t('settings.sections.sync.fileSync.smallFileThreshold.errors.invalid')
      )
      return
    }

    const size = parseInt(value)
    if (size < 1 || size > 1000) {
      setSmallFileThresholdError(
        t('settings.sections.sync.fileSync.smallFileThreshold.errors.range')
      )
      return
    }

    if (size >= effectiveMaxFileSizeLimit) {
      setSmallFileThresholdError(
        t('settings.sections.sync.fileSync.smallFileThreshold.errors.exceedsMax')
      )
      return
    }

    setSmallFileThresholdError(null)
    void updateFileSyncSetting({ smallFileThreshold: mbToBytes(size) }).then(
      () => setSmallFileThresholdDraft(active => (active === value ? null : active)),
      err => {
        log.error({ err }, 'Failed to change small-file threshold')
        setSmallFileThresholdDraft(active => (active === value ? null : active))
      }
    )
  }

  const handleMaxFileSizeLimitChange = (value: string) => {
    setMaxFileSizeLimitDraft(value)

    if (!value.trim()) {
      setMaxFileSizeLimitError(null)
      return
    }

    if (!/^\d+$/.test(value)) {
      setMaxFileSizeLimitError(t('settings.sections.sync.fileSync.maxFileSize.errors.invalid'))
      return
    }

    const size = parseInt(value)
    if (size < 1 || size > 10240) {
      setMaxFileSizeLimitError(t('settings.sections.sync.fileSync.maxFileSize.errors.range'))
      return
    }

    if (size <= effectiveSmallFileThreshold) {
      setMaxFileSizeLimitError(
        t('settings.sections.sync.fileSync.smallFileThreshold.errors.exceedsMax')
      )
      return
    }

    setMaxFileSizeLimitError(null)
    void updateFileSyncSetting({ maxFileSize: mbToBytes(size) }).then(
      () => setMaxFileSizeLimitDraft(active => (active === value ? null : active)),
      err => {
        log.error({ err }, 'Failed to change max-file-size setting')
        setMaxFileSizeLimitDraft(active => (active === value ? null : active))
      }
    )
  }

  const handleCacheQuotaChange = (value: string) => {
    setCacheQuotaDraft(value)

    if (!value.trim()) {
      setCacheQuotaError(null)
      return
    }

    if (!/^\d+$/.test(value)) {
      setCacheQuotaError(t('settings.sections.sync.fileSync.cacheQuota.errors.invalid'))
      return
    }

    const size = parseInt(value)
    if (size < 50 || size > 10240) {
      setCacheQuotaError(t('settings.sections.sync.fileSync.cacheQuota.errors.range'))
      return
    }

    setCacheQuotaError(null)
    void updateFileSyncSetting({ fileCacheQuotaPerDevice: mbToBytes(size) }).then(
      () => setCacheQuotaDraft(active => (active === value ? null : active)),
      err => {
        log.error({ err }, 'Failed to change file-cache quota')
        setCacheQuotaDraft(active => (active === value ? null : active))
      }
    )
  }

  const handleRetentionHoursChange = (value: string) => {
    setRetentionHoursDraft(value)

    if (!value.trim()) {
      setRetentionHoursError(null)
      return
    }

    if (!/^\d+$/.test(value)) {
      setRetentionHoursError(t('settings.sections.sync.fileSync.retentionPeriod.errors.invalid'))
      return
    }

    const hours = parseInt(value)
    if (hours < 1 || hours > 720) {
      setRetentionHoursError(t('settings.sections.sync.fileSync.retentionPeriod.errors.range'))
      return
    }

    setRetentionHoursError(null)
    void updateFileSyncSetting({ fileRetentionHours: hours }).then(
      () => setRetentionHoursDraft(active => (active === value ? null : active)),
      err => {
        log.error({ err }, 'Failed to change file-retention period')
        setRetentionHoursDraft(active => (active === value ? null : active))
      }
    )
  }

  const handlePickAutoSaveDir = async () => {
    setSavingAutoSaveDir(true)
    try {
      const picked = await commands.pickDirectory()
      // `null` ⇒ user cancelled the native picker; leave the setting unchanged.
      if (picked !== null) {
        await updateFileSyncSetting({ autoSaveDir: picked })
      }
    } catch (err) {
      log.error({ err }, 'Failed to set auto-save directory')
    } finally {
      setSavingAutoSaveDir(false)
    }
  }

  const handleClearAutoSaveDir = async () => {
    setSavingAutoSaveDir(true)
    try {
      // Empty string clears the setting back to managed storage.
      await updateFileSyncSetting({ autoSaveDir: '' })
    } catch (err) {
      log.error({ err }, 'Failed to clear auto-save directory')
    } finally {
      setSavingAutoSaveDir(false)
    }
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
          label={t('settings.sections.sync.syncEnabled.label')}
          description={t('settings.sections.sync.syncEnabled.description')}
        >
          <Switch
            id="sync-enabled"
            aria-label={t('settings.sections.sync.syncEnabled.label')}
            checked={syncEnabled}
            onCheckedChange={setSyncEnabled}
          />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.sync.autoSync.label')}
          description={t('settings.sections.sync.autoSync.description')}
        >
          <Switch
            id="auto-sync"
            aria-label={t('settings.sections.sync.autoSync.label')}
            checked={autoSyncEnabled}
            onCheckedChange={setAutoSyncEnabled}
            disabled={!syncEnabled}
          />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.sync.syncOnRestore.label')}
          description={t('settings.sections.sync.syncOnRestore.description')}
        >
          <Switch
            id="sync-on-restore"
            aria-label={t('settings.sections.sync.syncOnRestore.label')}
            checked={syncOnRestore}
            onCheckedChange={setSyncOnRestore}
            disabled={!syncEnabled}
          />
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

        {/* Auto-save directory for received files */}
        <SettingRow
          label={t('settings.sections.sync.fileSync.autoSaveDir.label')}
          description={t('settings.sections.sync.fileSync.autoSaveDir.description')}
        >
          <div className="flex flex-col items-end gap-2">
            {autoSaveDir ? (
              <span
                className="text-foreground max-w-64 truncate text-xs font-medium"
                title={autoSaveDir}
              >
                {autoSaveDir}
              </span>
            ) : (
              <span className="text-muted-foreground text-xs">
                {t('settings.sections.sync.fileSync.autoSaveDir.managed')}
              </span>
            )}
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={handlePickAutoSaveDir}
                disabled={!syncEnabled || !fileSyncEnabled || savingAutoSaveDir}
              >
                {t('settings.sections.sync.fileSync.autoSaveDir.choose')}
              </Button>
              {autoSaveDir && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleClearAutoSaveDir}
                  disabled={!syncEnabled || !fileSyncEnabled || savingAutoSaveDir}
                >
                  {t('settings.sections.sync.fileSync.autoSaveDir.clear')}
                </Button>
              )}
            </div>
          </div>
        </SettingRow>

        {/* Small file threshold */}
        <SettingRow
          label={t('settings.sections.sync.fileSync.smallFileThreshold.label')}
          description={t('settings.sections.sync.fileSync.smallFileThreshold.description')}
        >
          <Input
            aria-label={t('settings.sections.sync.fileSync.smallFileThreshold.label')}
            inputMode="numeric"
            value={smallFileThresholdValue}
            onChange={handleSmallFileThresholdChange}
            error={smallFileThresholdError ?? undefined}
            className="w-44 max-w-full"
            classNames={{
              field: 'h-9 rounded-lg bg-card',
              input: 'text-right text-sm tabular-nums',
              rightIcon: 'pr-3 text-xs',
            }}
            rightIcon={t('settings.sections.sync.fileSync.smallFileThreshold.unit')}
            disabled={!syncEnabled || !fileSyncEnabled}
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
            value={maxFileSizeLimitValue}
            onChange={handleMaxFileSizeLimitChange}
            error={maxFileSizeLimitError ?? undefined}
            className="w-44 max-w-full"
            classNames={{
              field: 'h-9 rounded-lg bg-card',
              input: 'text-right text-sm tabular-nums',
              rightIcon: 'pr-3 text-xs',
            }}
            rightIcon={t('settings.sections.sync.fileSync.maxFileSize.unit')}
            disabled={!syncEnabled || !fileSyncEnabled}
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
            value={cacheQuotaValue}
            onChange={handleCacheQuotaChange}
            error={cacheQuotaError ?? undefined}
            className="w-44 max-w-full"
            classNames={{
              field: 'h-9 rounded-lg bg-card',
              input: 'text-right text-sm tabular-nums',
              rightIcon: 'pr-3 text-xs',
            }}
            rightIcon={t('settings.sections.sync.fileSync.cacheQuota.unit')}
            disabled={!syncEnabled || !fileSyncEnabled}
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
            value={retentionHoursValue}
            onChange={handleRetentionHoursChange}
            error={retentionHoursError ?? undefined}
            className="w-44 max-w-full"
            classNames={{
              field: 'h-9 rounded-lg bg-card',
              input: 'text-right text-sm tabular-nums',
              rightIcon: 'pr-3 text-xs',
            }}
            rightIcon={t('settings.sections.sync.fileSync.retentionPeriod.unit')}
            disabled={!syncEnabled || !fileSyncEnabled}
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
            disabled={!syncEnabled || !fileSyncEnabled}
          />
        </SettingRow>
      </SettingGroup>
    </>
  )
}

export default SyncSection
