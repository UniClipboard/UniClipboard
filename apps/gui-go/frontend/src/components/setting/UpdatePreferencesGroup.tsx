import { TriangleAlert } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSetting } from '@/hooks/useSetting'
import { useUpdate } from '@/hooks/useUpdate'
import { createLogger } from '@/lib/logger'
import type { UpdateChannel } from '@/types/setting'
import { SettingAlert, SettingSelect, SettingSwitch } from './SettingControls'
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
          <SettingSwitch
            label={t('settings.sections.about.autoCheckUpdate.label')}
            checked={autoCheckUpdate}
            onCheckedChange={setAutoCheckUpdate}
          />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.about.autoDownloadUpdate.label')}
          description={
            autoCheckUpdate
              ? t('settings.sections.about.autoDownloadUpdate.description')
              : t('settings.sections.about.autoDownloadUpdate.disabledHint')
          }
        >
          <SettingSwitch
            label={t('settings.sections.about.autoDownloadUpdate.label')}
            checked={autoDownloadUpdate && autoCheckUpdate}
            onCheckedChange={setAutoDownloadUpdate}
            disabled={!autoCheckUpdate}
          />
        </SettingRow>

        <SettingRow
          label={t('settings.sections.about.updateChannel.label')}
          description={t('settings.sections.about.updateChannel.description')}
        >
          <SettingSelect
            label={t('settings.sections.about.updateChannel.label')}
            value={updateChannel ?? 'auto'}
            onValueChange={handleUpdateChannelChange}
            items={(['auto', 'stable', 'alpha'] as const).map(channel => ({
              value: channel,
              label: t(`settings.sections.about.updateChannel.${channel}`),
            }))}
          />
        </SettingRow>
      </SettingGroup>

      <SettingAlert
        open={alphaWarningOpen}
        onOpenChange={handleAlphaWarningOpenChange}
        title={t('settings.sections.about.updateChannel.alphaWarning.title')}
        description={t('settings.sections.about.updateChannel.alphaWarning.description')}
        confirmLabel={t('settings.sections.about.updateChannel.alphaWarning.confirm')}
        cancelLabel={t('settings.sections.about.updateChannel.alphaWarning.cancel')}
        media={<TriangleAlert className="size-5" />}
        // Stays open until the handler closes it.
        onConfirm={async () => handleConfirmAlphaChannel()}
      />
    </>
  )
}
