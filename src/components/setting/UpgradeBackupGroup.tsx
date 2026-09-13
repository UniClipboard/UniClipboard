import { Loader2, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import * as storageApi from '@/api/storage'
import type { UpgradeBackup } from '@/api/storage'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Button,
} from '@/components/ui'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'
import { formatFileSize } from '@/utils/formatters'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'

const log = createLogger('upgrade-backups')
const I18N = 'settings.sections.storage.upgradeBackups'

export function UpgradeBackupGroup() {
  const { t, i18n } = useTranslation()
  const [backups, setBackups] = useState<UpgradeBackup[]>([])
  const [loading, setLoading] = useState(true)
  const [loadFailed, setLoadFailed] = useState(false)
  const [selected, setSelected] = useState<UpgradeBackup | null>(null)
  const [deleting, setDeleting] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    setLoadFailed(false)
    try {
      setBackups(await storageApi.listUpgradeBackups())
    } catch (error) {
      log.error({ err: error }, 'Failed to list upgrade backups')
      setLoadFailed(true)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const handleDelete = async () => {
    if (!selected) return
    setDeleting(true)
    try {
      await storageApi.deleteUpgradeBackup(selected.id, true)
      setBackups(current => current.filter(backup => backup.id !== selected.id))
      setSelected(null)
      toast.success(t(`${I18N}.deleted`))
    } catch (error) {
      log.error({ err: error }, 'Failed to delete upgrade backup')
      toast.error(t(`${I18N}.deleteFailed`))
    } finally {
      setDeleting(false)
    }
  }

  const versionText = (backup: UpgradeBackup) => {
    const source = backup.sourceProduct ?? backup.sourceEngine ?? t(`${I18N}.unknownVersion`)
    return t(`${I18N}.versionChange`, { source, target: backup.targetProduct })
  }

  return (
    <>
      <SettingGroup title={t(`${I18N}.label`)}>
        <SettingRow description={t(`${I18N}.description`)}>
          <span className="text-ui-caption text-muted-foreground">
            {t(`${I18N}.count`, { count: backups.length })}
          </span>
        </SettingRow>

        {loading && (
          <SettingRow description={t(`${I18N}.loading`)}>
            <Loader2 aria-hidden="true" className="size-4 animate-spin text-muted-foreground" />
          </SettingRow>
        )}

        {!loading && loadFailed && (
          <SettingRow description={t(`${I18N}.loadFailed`)}>
            <Button variant="outline" size="sm" onClick={() => void load()}>
              {t(`${I18N}.retry`)}
            </Button>
          </SettingRow>
        )}

        {!loading && !loadFailed && backups.length === 0 && (
          <SettingRow description={t(`${I18N}.empty`)} />
        )}

        {!loading &&
          !loadFailed &&
          backups.map(backup => (
            <SettingRow
              key={backup.id}
              label={new Date(backup.createdAtMs).toLocaleString(i18n.resolvedLanguage)}
              description={`${versionText(backup)} · ${formatFileSize(backup.sizeBytes)}`}
            >
              <Button
                variant="ghost"
                size="icon"
                aria-label={t(`${I18N}.deleteLabel`, {
                  date: new Date(backup.createdAtMs).toLocaleString(i18n.resolvedLanguage),
                })}
                onClick={() => setSelected(backup)}
              >
                <Trash2 aria-hidden="true" className="size-4" />
              </Button>
            </SettingRow>
          ))}
      </SettingGroup>

      <AlertDialog
        open={selected !== null}
        onOpenChange={open => {
          if (!open && !deleting) setSelected(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t(`${I18N}.confirmTitle`)}</AlertDialogTitle>
            <AlertDialogDescription>{t(`${I18N}.confirmDescription`)}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>{t(`${I18N}.cancel`)}</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              disabled={deleting}
              onClick={event => {
                event.preventDefault()
                void handleDelete()
              }}
            >
              {deleting ? t(`${I18N}.deleting`) : t(`${I18N}.delete`)}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
