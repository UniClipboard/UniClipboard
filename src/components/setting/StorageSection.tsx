import { useTranslation } from 'react-i18next'
import { useSetting } from '@/hooks/useSetting'
import { ConfigBackupGroup } from './ConfigBackupGroup'
import { RetentionPolicyGroup } from './RetentionPolicyGroup'
import { SearchIndexGroup } from './SearchIndexGroup'
import { StorageDataGroup } from './StorageDataGroup'
import { StorageUsageGroup } from './StorageUsageGroup'
import { useStorageStats } from './useStorageStats'

export default function StorageSection() {
  const { t } = useTranslation()
  const { error } = useSetting()
  const { stats, loading, error: statsError, refresh } = useStorageStats()
  if (error)
    return (
      <div className="text-destructive py-4">
        {t('settings.sections.storage.loadError')} {error}
      </div>
    )
  return (
    <div className="flex min-w-0 flex-col gap-8">
      <StorageUsageGroup stats={stats} loading={loading} error={statsError} onRefresh={refresh} />
      <SearchIndexGroup />
      <RetentionPolicyGroup />
      <StorageDataGroup onChanged={refresh} />
      <ConfigBackupGroup />
    </div>
  )
}
