import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import * as storageApi from '@/api/storage'
import { Button, Switch } from '@/components/ui'
import {
  readDeleteConfirmationEnabled,
  setDeleteConfirmationEnabled,
} from '@/lib/delete-confirmation-preference'
import { createLogger } from '@/lib/logger'
import ClearHistoryDialog from './ClearHistoryDialog'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
const log = createLogger('storage-data')

const handleOpenDataDir = async () => {
  try {
    await storageApi.openDataDirectory()
  } catch (err) {
    log.error({ err }, 'Failed to open data directory')
  }
}

export function StorageDataGroup({ onChanged: loadStats }: { onChanged: () => Promise<void> }) {
  const { t } = useTranslation()
  const [clearingCache, setClearingCache] = useState(false)
  const [clearingHistory, setClearingHistory] = useState(false)
  const [showClearHistoryDialog, setShowClearHistoryDialog] = useState(false)
  const [confirmBeforeDelete, setConfirmBeforeDelete] = useState(() =>
    readDeleteConfirmationEnabled()
  )

  const handleClearCache = async () => {
    setClearingCache(true)
    try {
      await storageApi.clearCache(true)
      await loadStats()
    } catch (err) {
      log.error({ err }, 'Failed to clear cache')
    } finally {
      setClearingCache(false)
    }
  }

  const handleClearHistory = async () => {
    setClearingHistory(true)
    try {
      await storageApi.clearAllClipboardHistory()
      // The history view re-queries via useLiveSearch on next mount; no Redux
      // browse list to reset anymore.
      await loadStats()
    } catch (err) {
      log.error({ err }, 'Failed to clear history')
      throw err
    } finally {
      setClearingHistory(false)
    }
  }

  return (
    <>
      <SettingGroup title={t('settings.sections.storage.dataDirectory.label')}>
        <SettingRow
          label={t('settings.sections.storage.confirmBeforeDelete.label')}
          description={t('settings.sections.storage.confirmBeforeDelete.description')}
        >
          <Switch
            aria-label={t('settings.sections.storage.confirmBeforeDelete.label')}
            checked={confirmBeforeDelete}
            onCheckedChange={checked => {
              setConfirmBeforeDelete(checked)
              setDeleteConfirmationEnabled(checked)
            }}
          />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.clearCache.label')}
          description={t('settings.sections.storage.clearCache.description')}
        >
          <Button variant="outline" size="sm" onClick={handleClearCache} disabled={clearingCache}>
            {clearingCache
              ? t('settings.sections.storage.clearCache.clearing')
              : t('settings.sections.storage.clearCache.button')}
          </Button>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.clearHistory.label')}
          description={t('settings.sections.storage.clearHistory.description')}
        >
          <Button
            variant="destructive"
            size="sm"
            onClick={() => setShowClearHistoryDialog(true)}
            disabled={clearingHistory}
          >
            {clearingHistory
              ? t('settings.sections.storage.clearHistory.clearing')
              : t('settings.sections.storage.clearHistory.button')}
          </Button>
        </SettingRow>

        <SettingRow
          label={t('settings.sections.storage.dataDirectory.label')}
          description={t('settings.sections.storage.dataDirectory.description')}
        >
          <Button variant="outline" size="sm" onClick={handleOpenDataDir}>
            {t('settings.sections.storage.dataDirectory.button')}
          </Button>
        </SettingRow>
      </SettingGroup>

      <ClearHistoryDialog
        open={showClearHistoryDialog}
        onOpenChange={setShowClearHistoryDialog}
        onConfirm={handleClearHistory}
      />
    </>
  )
}
