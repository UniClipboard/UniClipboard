import { useTranslation } from 'react-i18next'
import type { SpaceMember } from '@/api/daemon/members'
import { deriveBadgeKind, derivePeerStatusTone } from '@/components/device/connection-channel-utils'
import type { DeviceRowStatus } from '@/components/device/device-trust-view'
import { usePeerSyncPreferences } from '@/hooks/usePeerSyncPreferences'
import { useSettingSelector } from '@/hooks/useSetting'
import { DevicesKitPeerAddress } from '@/pages/devices-kit/DevicesKitPeerAddress'
import { Button, Disclosure, EntityHeader, SettingsGroup, SettingsRow, Text } from '@/ui-kit'
import { deviceIcon, dotTone } from './devices-kit-model'
import { DevicesKitCopyButton } from './DevicesKitCopyButton'
import { DevicesKitFact } from './DevicesKitFact'
import { DevicesKitSyncPreferences } from './DevicesKitSyncPreferences'

/** A paired device (design DV1): status and channel, sync preferences, and the connection facts. */
export function DevicesKitPeerDetail({
  device,
  status,
  onUnpair,
}: {
  device: SpaceMember
  status?: DeviceRowStatus
  onUnpair: () => void
}) {
  const { t } = useTranslation()
  const sync = usePeerSyncPreferences(device.peerId)
  const globalSyncOff = useSettingSelector(({ setting }) => setting?.sync.syncEnabled === false)
  const globalFileSyncOff = useSettingSelector(
    ({ setting }) => setting?.fileSync?.fileSyncEnabled === false
  )
  const lanOnly = useSettingSelector(
    ({ setting }) => setting?.network?.allowRelayFallback === false
  )
  const name = device.deviceName || t('devices.list.labels.unknownDevice')
  const channelKind = deriveBadgeKind(device.channel ?? 'unknown', lanOnly)
  const channelLabel = t(`devices.list.channel.${channelKind}`)
  const protection = sync.protectionStatus
  const unprotected = protection !== undefined && protection !== 'protected'
  const description = [status?.description, status?.detail].filter(Boolean).join(' ')

  return (
    <>
      <EntityHeader
        icon={deviceIcon(device.deviceName)}
        title={name}
        status={{
          text:
            status?.label ??
            t(device.connected ? 'devices.list.status.online' : 'devices.list.status.offline'),
          tone: status
            ? dotTone[status.tone ?? 'warning']
            : dotTone[derivePeerStatusTone(device.channel ?? 'unknown', device.connected)],
          detail: channelLabel,
        }}
        actions={
          <Button
            data-testid="device-unpair"
            variant="destructive"
            icon="remove"
            onClick={onUnpair}
          >
            {t('devices.kit.unpairEllipsis')}
          </Button>
        }
      />
      {description && (
        <span data-testid="device-status-description">
          <Text role="caption" tone="secondary">
            {description}
          </Text>
        </span>
      )}
      <DevicesKitSyncPreferences
        deviceName={name}
        sync={sync}
        globalSyncOff={globalSyncOff}
        globalFileSyncOff={globalFileSyncOff}
      />
      <DevicesKitPeerAddress key={device.peerId} deviceId={device.peerId} />
      <Disclosure label={t('devices.settings.sections.connection')} defaultOpen={unprotected}>
        <SettingsGroup>
          <DevicesKitFact
            label={t('devices.panel.fields.peerId')}
            value={device.peerId}
            monospace
            trailing={
              <DevicesKitCopyButton value={device.peerId} label={t('devices.kit.copyPeerId')} />
            }
          />
          <SettingsRow
            label={t('devices.settings.fields.channel')}
            description={t(`devices.list.channel.tooltip.${channelKind}`)}
          >
            <Text role="callout" tone="secondary">
              {channelLabel}
            </Text>
          </SettingsRow>
          {unprotected && (
            <SettingsRow
              label={t('devices.protection.label')}
              description={
                protection === 'awaiting_readmission'
                  ? t('devices.protection.upgradePeerHelp')
                  : undefined
              }
            >
              <Text role="callout" tone="critical">
                {t(`devices.protection.status.${protection}`)}
              </Text>
            </SettingsRow>
          )}
          {device.connectionAddress && (
            <DevicesKitFact
              label={t('devices.settings.fields.address')}
              value={device.connectionAddress}
              monospace
            />
          )}
        </SettingsGroup>
      </Disclosure>
    </>
  )
}
