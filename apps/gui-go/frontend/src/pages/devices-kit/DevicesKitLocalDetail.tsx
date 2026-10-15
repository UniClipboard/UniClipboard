import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { LocalDeviceInfo } from '@/api/daemon/members'
import type { DeviceRowStatus } from '@/components/device/device-trust-view'
import { getPlatformLabel } from '@/components/device/device-utils'
import { useAppVersion } from '@/hooks/useAppVersion'
import { useRebuildSpace } from '@/hooks/useRebuildSpace'
import { useSettingSelector } from '@/hooks/useSetting'
import { AlertDialog, Button, Disclosure, EntityHeader, SettingsGroup, SettingsRow } from '@/ui-kit'
import { deviceIcon } from './devices-kit-model'
import { DevicesKitCopyButton } from './DevicesKitCopyButton'
import { DevicesKitFact } from './DevicesKitFact'
import { DevicesKitLocalSyncRow } from './DevicesKitLocalSyncRow'

/**
 * This device (design DV2): its sync switches (global settings), its profile, and Rebuild Space
 * confirmed by typing RESET.
 */
export function DevicesKitLocalDetail({
  localDevice,
  memberCount,
  status,
  onRebuildSucceeded,
}: {
  localDevice: LocalDeviceInfo
  memberCount: number
  status?: DeviceRowStatus
  onRebuildSucceeded: () => void
}) {
  const { t } = useTranslation()
  const appVersion = useAppVersion()
  const platform = getPlatformLabel()
  const settingsLoaded = useSettingSelector(context => context.setting !== null)
  const syncActive = useSettingSelector(
    ({ setting }) => setting !== null && setting.sync.syncEnabled !== false
  )
  const rebuild = useRebuildSpace(onRebuildSucceeded)
  const [rebuildOpen, setRebuildOpen] = useState(false)
  const needsAttention = status != null && !['online', 'paused', 'unknown'].includes(status.kind)

  return (
    <>
      <EntityHeader
        icon={deviceIcon(localDevice.deviceName)}
        title={localDevice.deviceName}
        badge={t('devices.panel.localBadge')}
        status={{
          text:
            status?.label ??
            (!settingsLoaded
              ? t('common.loading')
              : t(syncActive ? 'devices.thisDevice.syncActive' : 'devices.thisDevice.syncPaused')),
          tone: needsAttention ? 'warning' : syncActive ? 'positive' : 'neutral',
        }}
        description={status?.description}
      />
      <SettingsGroup>
        <DevicesKitLocalSyncRow kind="sync" />
        <DevicesKitLocalSyncRow kind="file" />
      </SettingsGroup>
      <Disclosure label={t('devices.panel.profile.title')}>
        <SettingsGroup>
          <DevicesKitFact
            label={t('devices.panel.fields.peerId')}
            value={localDevice.peerId}
            monospace
            trailing={
              <DevicesKitCopyButton
                value={localDevice.peerId}
                label={t('devices.kit.copyPeerId')}
              />
            }
          />
          {platform && (
            <DevicesKitFact label={t('devices.panel.profile.platform')} value={platform} />
          )}
          {appVersion && (
            <DevicesKitFact
              label={t('devices.panel.profile.version')}
              value={`v${appVersion}`}
              monospace
            />
          )}
          <DevicesKitFact
            label={t('devices.panel.profile.space')}
            value={t('devices.panel.profile.memberCount', { count: memberCount })}
          />
        </SettingsGroup>
      </Disclosure>
      <SettingsGroup title={t('devices.panel.danger.title')}>
        <SettingsRow
          label={t('devices.panel.danger.reset')}
          description={t('devices.panel.danger.description')}
        >
          <Button
            variant="destructive"
            onClick={() => {
              rebuild.reset()
              setRebuildOpen(true)
            }}
          >
            {t('devices.kit.rebuildEllipsis')}
          </Button>
        </SettingsRow>
      </SettingsGroup>
      <AlertDialog
        open={rebuildOpen}
        onOpenChange={open => {
          if (!rebuild.rebuilding) setRebuildOpen(open)
        }}
        title={t('devices.panel.danger.modal.title')}
        description={t('devices.panel.danger.modal.warning')}
        field={{
          label: t('devices.panel.danger.modal.confirmPrompt'),
          value: rebuild.confirmation,
          onValueChange: rebuild.setConfirmation,
          placeholder: t('devices.panel.danger.modal.confirmPlaceholder'),
        }}
        confirmLabel={t('devices.panel.danger.modal.confirm')}
        cancelLabel={t('devices.panel.danger.modal.cancel')}
        destructive
        confirmDisabled={!rebuild.confirmed}
        busyLabel={rebuild.rebuilding ? t('devices.panel.danger.modal.resetting') : undefined}
        error={rebuild.errorKey ? t(rebuild.errorKey) : undefined}
        onConfirm={async () => {
          if (await rebuild.rebuild()) setRebuildOpen(false)
        }}
      />
    </>
  )
}
