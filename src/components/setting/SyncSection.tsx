import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
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

const SyncSection: React.FC = () => {
  const { t } = useTranslation()
  // Use setting context
  const { setting, error, updateSyncSetting } = useSetting()

  // Local state for UI display - initialize from setting to avoid flash
  const [autoSync, setAutoSync] = useState(setting?.sync.auto_sync ?? true)
  const [syncFrequency, setSyncFrequency] = useState<string>(
    setting?.sync.sync_frequency ?? 'realtime'
  )

  const [maxFileSize, setMaxFileSize] = useState(setting?.sync.max_file_size_mb ?? 10)
  const [maxFileSizeError, setMaxFileSizeError] = useState<string | null>(null)

  // Sync frequency options
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
      setAutoSync(setting.sync.auto_sync)
      setSyncFrequency(setting.sync.sync_frequency)

      setMaxFileSize(setting.sync.max_file_size_mb)
    }
  }, [setting])

  // Handle auto sync switch change
  const handleAutoSyncChange = (checked: boolean) => {
    setAutoSync(checked)
    updateSyncSetting({ auto_sync: checked })
  }

  // Handle sync frequency change
  const handleSyncFrequencyChange = (value: string) => {
    setSyncFrequency(value)
    // TODO: 后端 SyncFrequency 只支持 'realtime' | 'interval'
    // UI 选项包括更多值 ('30s', '1m', '5m', '15m')，需要后续扩展后端类型
    // 暂时使用类型断言让编译通过
    updateSyncSetting({ sync_frequency: value as 'realtime' | 'interval' })
  }

  // Handle max file size change
  const handleMaxFileSizeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    // If input is empty, allow user to continue typing
    if (!value.trim()) {
      setMaxFileSizeError(null)
      setMaxFileSize(0)
      return
    }

    // Check if it's a number
    if (!/^\d+$/.test(value)) {
      setMaxFileSizeError(t('settings.sections.sync.maxFileSize.errors.invalid'))
      setMaxFileSize(parseInt(value) || 0)
      return
    }

    const size = parseInt(value)
    setMaxFileSize(size)

    // Validate range (1-50 MB)
    if (size < 1 || size > 50) {
      setMaxFileSizeError(t('settings.sections.sync.maxFileSize.errors.range'))
      return
    }

    // Validation passed
    setMaxFileSizeError(null)
    updateSyncSetting({ max_file_size_mb: size })
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
        <Select value={syncFrequency} onValueChange={handleSyncFrequencyChange}>
          <SelectTrigger className="w-52">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {syncFrequencyOptions.map(option => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingRow>

      <SettingRow
        label={t('settings.sections.sync.maxFileSize.label')}
        description={t('settings.sections.sync.maxFileSize.description')}
      >
        <div className="flex flex-col items-end gap-1">
          <div className="flex items-center gap-2">
            <Input
              type="text"
              value={maxFileSize.toString()}
              onChange={handleMaxFileSizeChange}
              className={maxFileSizeError ? 'border-red-500 w-32' : 'w-32'}
            />
            <span className="text-sm text-muted-foreground">MB</span>
          </div>
          {maxFileSizeError && <p className="text-xs text-red-500">{maxFileSizeError}</p>}
        </div>
      </SettingRow>
    </SettingGroup>
  )
}

export default SyncSection
