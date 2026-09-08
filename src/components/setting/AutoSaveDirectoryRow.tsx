import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { SettingRow } from './SettingRow'
const log = createLogger('file-auto-save-directory')

export function AutoSaveDirectoryRow({ disabled }: { disabled: boolean }) {
  const { t } = useTranslation()
  const { setting, updateFileSyncSetting } = useSetting()
  // Auto-save directory for inbound files (null/undefined ⇒ managed storage).
  const autoSaveDir = setting?.fileSync?.autoSaveDir ?? null
  const [savingAutoSaveDir, setSavingAutoSaveDir] = useState(false)

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

  return (
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
            disabled={disabled || savingAutoSaveDir}
          >
            {t('settings.sections.sync.fileSync.autoSaveDir.choose')}
          </Button>
          {autoSaveDir && (
            <Button
              variant="ghost"
              size="sm"
              onClick={handleClearAutoSaveDir}
              disabled={disabled || savingAutoSaveDir}
            >
              {t('settings.sections.sync.fileSync.autoSaveDir.clear')}
            </Button>
          )}
        </div>
      </div>
    </SettingRow>
  )
}
