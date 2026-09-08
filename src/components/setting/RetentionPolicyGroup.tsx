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
import type { RetentionPolicy } from '@/types/setting'
import {
  DEFAULT_RETENTION_POLICY,
  SECONDS_PER_DAY,
  RETENTION_DAYS_OPTIONS,
  MAX_ITEMS_OPTIONS,
  getByAgeSecs,
  getByCountItems,
  setByAgeRule,
  setByCountRule,
  clearByCountRule,
  resolveMaxItemsCount,
} from './retention-policy'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'

export function RetentionPolicyGroup() {
  const { t } = useTranslation()
  const { setting, updateRetentionPolicy } = useSetting()
  const [retentionPolicy, setRetentionPolicy] = useOptimisticSetting<RetentionPolicy>(
    setting?.retentionPolicy ?? DEFAULT_RETENTION_POLICY,
    next => updateRetentionPolicy(next),
    { failureLog: 'Failed to update retention policy' }
  )
  const enabled = retentionPolicy.enabled
  const skipPinned = retentionPolicy.skipPinned
  const ageSecs = getByAgeSecs(retentionPolicy.rules)
  const retentionDays =
    RETENTION_DAYS_OPTIONS.find(
      option => option.days === Math.round((ageSecs ?? 0) / SECONDS_PER_DAY)
    )?.value ?? '30'
  const countItems = getByCountItems(retentionPolicy.rules)
  const maxItems =
    countItems === null
      ? 'unlimited'
      : (MAX_ITEMS_OPTIONS.find(option => option.count === countItems)?.value ?? '500')

  const handleEnabledChange = (checked: boolean) => {
    setRetentionPolicy({ ...retentionPolicy, enabled: checked })
  }

  const handleRetentionDaysChange = (value: string) => {
    const days = RETENTION_DAYS_OPTIONS.find(o => o.value === value)?.days ?? 30
    setRetentionPolicy({
      ...retentionPolicy,
      rules: setByAgeRule(retentionPolicy.rules, days),
    })
  }

  const handleMaxItemsChange = (value: string) => {
    const count = resolveMaxItemsCount(value)
    setRetentionPolicy({
      ...retentionPolicy,
      rules:
        count === null
          ? clearByCountRule(retentionPolicy.rules)
          : setByCountRule(retentionPolicy.rules, count),
    })
  }

  const handleSkipPinnedChange = (checked: boolean) => {
    setRetentionPolicy({ ...retentionPolicy, skipPinned: checked })
  }

  return (
    <SettingGroup title={t('settings.sectionHeadings.historyRetention')}>
      <SettingRow
        label={t('settings.sections.storage.autoClearHistory.label')}
        description={t('settings.sections.storage.autoClearHistory.description')}
      >
        <Switch checked={enabled} onCheckedChange={handleEnabledChange} />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.storage.historyRetention.label')}
        description={t('settings.sections.storage.historyRetention.description')}
      >
        <Select value={retentionDays} onValueChange={handleRetentionDaysChange} disabled={!enabled}>
          <SelectTrigger className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {RETENTION_DAYS_OPTIONS.map(opt => (
              <SelectItem key={opt.value} value={opt.value}>
                {t('settings.sections.storage.historyRetention.days', { days: opt.days })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingRow>

      <SettingRow
        label={t('settings.sections.storage.maxHistoryItems.label')}
        description={t('settings.sections.storage.maxHistoryItems.description')}
      >
        <Select value={maxItems} onValueChange={handleMaxItemsChange} disabled={!enabled}>
          <SelectTrigger className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {MAX_ITEMS_OPTIONS.map(opt => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.count === null
                  ? t('settings.sections.storage.maxHistoryItems.unlimited')
                  : t('settings.sections.storage.maxHistoryItems.items', { count: opt.count })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingRow>

      <SettingRow
        label={t('settings.sections.storage.skipPinned.label')}
        description={t('settings.sections.storage.skipPinned.description')}
      >
        <Switch checked={skipPinned} onCheckedChange={handleSkipPinnedChange} disabled={!enabled} />
      </SettingRow>
    </SettingGroup>
  )
}
