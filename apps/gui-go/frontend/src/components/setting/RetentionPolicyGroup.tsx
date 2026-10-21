import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
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
import { SettingSelect, SettingSwitch } from './SettingControls'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'

function changedRetentionFields(
  shown: RetentionPolicy,
  next: RetentionPolicy
): Partial<RetentionPolicy> {
  const changed: Partial<RetentionPolicy> = {}
  if (next.enabled !== shown.enabled) changed.enabled = next.enabled
  if (next.skipPinned !== shown.skipPinned) changed.skipPinned = next.skipPinned
  if (next.evaluation !== shown.evaluation) changed.evaluation = next.evaluation
  // The daemon replaces the rule list as a whole.
  if (JSON.stringify(next.rules) !== JSON.stringify(shown.rules)) changed.rules = next.rules
  return changed
}

export function RetentionPolicyGroup() {
  const { t } = useTranslation()
  const { setting, updateRetentionPolicy } = useSetting()
  // Each control edits one field of the whole policy it shows; only the fields that differ from the
  // shown policy are saved, so a concurrent change of another field is not overwritten.
  const shownRef = useRef<RetentionPolicy>(DEFAULT_RETENTION_POLICY)
  const [retentionPolicy, setRetentionPolicy] = useOptimisticSetting<RetentionPolicy>(
    setting?.retentionPolicy ?? DEFAULT_RETENTION_POLICY,
    next => updateRetentionPolicy(changedRetentionFields(shownRef.current, next)),
    { failureLog: 'Failed to update retention policy' }
  )
  shownRef.current = retentionPolicy
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
        <SettingSwitch
          label={t('settings.sections.storage.autoClearHistory.label')}
          checked={enabled}
          onCheckedChange={handleEnabledChange}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.storage.historyRetention.label')}
        description={t('settings.sections.storage.historyRetention.description')}
      >
        <SettingSelect
          label={t('settings.sections.storage.historyRetention.label')}
          value={retentionDays}
          onValueChange={handleRetentionDaysChange}
          disabled={!enabled}
          items={RETENTION_DAYS_OPTIONS.map(opt => ({
            value: opt.value,
            label: t('settings.sections.storage.historyRetention.days', { days: opt.days }),
          }))}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.storage.maxHistoryItems.label')}
        description={t('settings.sections.storage.maxHistoryItems.description')}
      >
        <SettingSelect
          label={t('settings.sections.storage.maxHistoryItems.label')}
          value={maxItems}
          onValueChange={handleMaxItemsChange}
          disabled={!enabled}
          items={MAX_ITEMS_OPTIONS.map(opt => ({
            value: opt.value,
            label:
              opt.count === null
                ? t('settings.sections.storage.maxHistoryItems.unlimited')
                : t('settings.sections.storage.maxHistoryItems.items', { count: opt.count }),
          }))}
        />
      </SettingRow>

      <SettingRow
        label={t('settings.sections.storage.skipPinned.label')}
        description={t('settings.sections.storage.skipPinned.description')}
      >
        <SettingSwitch
          label={t('settings.sections.storage.skipPinned.label')}
          checked={skipPinned}
          onCheckedChange={handleSkipPinnedChange}
          disabled={!enabled}
        />
      </SettingRow>
    </SettingGroup>
  )
}
