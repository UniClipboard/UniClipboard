import { TriangleAlert } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Switch,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui'
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogCancel,
  AlertDialogAction,
} from '@/components/ui/alert-dialog'
import { useSetting } from '@/hooks/useSetting'
import { useUpdate } from '@/hooks/useUpdate'
import { createLogger } from '@/lib/logger'
import type { UpdateChannel } from '@/types/setting'
import { SettingGroup } from './SettingGroup'
import { SettingRow } from './SettingRow'
import { useOptimisticSetting } from './useOptimisticSetting'
const log = createLogger('update-preferences')
function normalizeUpdateChannel(value: string): UpdateChannel | null {
  return value === 'auto' ? null : (value as UpdateChannel)
}

export function UpdatePreferencesGroup() {
  const { t } = useTranslation()
  const { setting, updateGeneralSetting } = useSetting()
  const { checkForUpdates } = useUpdate()
  const [alphaWarningOpen, setAlphaWarningOpen] = useState(false)
  const [autoCheckUpdate, setAutoCheckUpdate] = useOptimisticSetting(
    setting?.general.autoCheckUpdate ?? true,
    next => updateGeneralSetting({ autoCheckUpdate: next }),
    { failureLog: 'Failed to change auto-check-update setting' }
  )
  const [autoDownloadUpdate, setAutoDownloadUpdate] = useOptimisticSetting(
    setting?.general.autoDownloadUpdate ?? false,
    next => updateGeneralSetting({ autoDownloadUpdate: next }),
    { failureLog: 'Failed to change auto-download-update setting' }
  )
  // The channel persists like the others, then kicks off a background update
  // check for the newly-selected channel (best-effort — a failed check does not
  // revert the channel).
  const [updateChannel, setUpdateChannel] = useOptimisticSetting<UpdateChannel | null>(
    setting?.general.updateChannel ?? null,
    async next => {
      await updateGeneralSetting({ updateChannel: next })
      checkForUpdates(next).catch(err => log.error({ err }, 'Failed to check for updates'))
    },
    { failureLog: 'Failed to change update channel' }
  )
  const pendingUpdateChannelRef = useRef<UpdateChannel | null>(null)
  const handleUpdateChannelChange = (value: string) => {
    const newChannel = normalizeUpdateChannel(value)
    if (newChannel === updateChannel) return

    if (newChannel === 'alpha' && updateChannel !== 'alpha') {
      pendingUpdateChannelRef.current = newChannel
      setAlphaWarningOpen(true)
      return
    }

    setUpdateChannel(newChannel)
  }

  const handleAlphaWarningOpenChange = (open: boolean) => {
    setAlphaWarningOpen(open)
    if (!open) {
      pendingUpdateChannelRef.current = null
    }
  }

  const handleConfirmAlphaChannel = () => {
    const channel = pendingUpdateChannelRef.current
    setAlphaWarningOpen(false)
    pendingUpdateChannelRef.current = null

    if (channel !== 'alpha') return
    setUpdateChannel(channel)
  }

  return (
    <>
      <SettingGroup title={t('settings.sections.about.updatesTitle')}>
        <SettingRow
          label={t('settings.sections.about.autoCheckUpdate.label')}
          description={t('settings.sections.about.autoCheckUpdate.description')}
        >
          <Switch checked={autoCheckUpdate} onCheckedChange={setAutoCheckUpdate} />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.about.autoDownloadUpdate.label')}
          description={
            autoCheckUpdate
              ? t('settings.sections.about.autoDownloadUpdate.description')
              : t('settings.sections.about.autoDownloadUpdate.disabledHint')
          }
        >
          <Switch
            checked={autoDownloadUpdate && autoCheckUpdate}
            onCheckedChange={setAutoDownloadUpdate}
            disabled={!autoCheckUpdate}
          />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.about.updateChannel.label')}
          description={t('settings.sections.about.updateChannel.description')}
        >
          <Select value={updateChannel ?? 'auto'} onValueChange={handleUpdateChannelChange}>
            <SelectTrigger className="w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="auto">
                {t('settings.sections.about.updateChannel.auto')}
              </SelectItem>
              <SelectItem value="stable">
                {t('settings.sections.about.updateChannel.stable')}
              </SelectItem>
              <SelectItem value="alpha">
                {t('settings.sections.about.updateChannel.alpha')}
              </SelectItem>
            </SelectContent>
          </Select>
        </SettingRow>
      </SettingGroup>

      <AlertDialog open={alphaWarningOpen} onOpenChange={handleAlphaWarningOpenChange}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogMedia className="bg-amber-500/10 text-amber-600 dark:text-amber-400">
              <TriangleAlert className="size-5" />
            </AlertDialogMedia>
            <AlertDialogTitle>
              {t('settings.sections.about.updateChannel.alphaWarning.title')}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('settings.sections.about.updateChannel.alphaWarning.description')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>
              {t('settings.sections.about.updateChannel.alphaWarning.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={event => {
                event.preventDefault()
                handleConfirmAlphaChannel()
              }}
            >
              {t('settings.sections.about.updateChannel.alphaWarning.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
