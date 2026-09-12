import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useSearchIndex } from './useSearchIndex'

export function SearchIndexGroup() {
  const { t } = useTranslation()
  const {
    status: searchStatus,
    rebuilding: rebuildingIndex,
    rebuild: handleRebuildIndex,
  } = useSearchIndex()
  return (
    <SettingGroup title={t('settings.sections.storage.searchIndex.label')}>
      <SettingRow
        label={t('settings.sections.storage.searchIndex.status')}
        description={t('settings.sections.storage.searchIndex.statusDescription')}
      >
        <div className="flex items-center gap-2">
          <span
            className={`inline-block size-2 rounded-full ${
              searchStatus?.state === 'ready'
                ? 'bg-green-500'
                : searchStatus?.state === 'rebuilding'
                  ? 'bg-yellow-500 animate-pulse'
                  : 'bg-muted-foreground/40'
            }`}
          />
          <span className="text-ui-body text-muted-foreground">
            {searchStatus?.state === 'ready'
              ? t('settings.sections.storage.searchIndex.ready')
              : searchStatus?.state === 'rebuilding'
                ? t('settings.sections.storage.searchIndex.rebuilding')
                : t('settings.sections.storage.searchIndex.unavailable')}
          </span>
        </div>
      </SettingRow>

      <SettingRow
        label={t('settings.sections.storage.searchIndex.lastRebuilt')}
        description={t('settings.sections.storage.searchIndex.lastRebuiltDescription')}
      >
        <span className="text-ui-body text-muted-foreground tabular-nums">
          {searchStatus?.lastRebuildCompletedAtMs
            ? new Date(searchStatus.lastRebuildCompletedAtMs).toLocaleString()
            : t('settings.sections.storage.searchIndex.never')}
        </span>
      </SettingRow>

      <SettingRow
        label={t('settings.sections.storage.searchIndex.rebuild')}
        description={t('settings.sections.storage.searchIndex.rebuildDescription')}
      >
        <Button variant="outline" size="sm" onClick={handleRebuildIndex} disabled={rebuildingIndex}>
          {rebuildingIndex
            ? t('settings.sections.storage.searchIndex.rebuildingButton')
            : t('settings.sections.storage.searchIndex.rebuildButton')}
        </Button>
      </SettingRow>
    </SettingGroup>
  )
}
