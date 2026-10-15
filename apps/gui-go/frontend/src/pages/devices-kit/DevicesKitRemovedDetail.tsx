import { useTranslation } from 'react-i18next'
import type { DeviceTrustRelationship } from '@/api/daemon/device-trust'
import { EntityHeader, InlineNotice, SettingsGroup } from '@/ui-kit'
import { deviceIcon } from './devices-kit-model'
import { DevicesKitCopyButton } from './DevicesKitCopyButton'
import { DevicesKitFact } from './DevicesKitFact'

/** A removed device whose removal notice has not reached it yet. */
export function DevicesKitRemovedDetail({ device }: { device: DeviceTrustRelationship }) {
  const { t } = useTranslation()
  return (
    <>
      <EntityHeader
        icon={deviceIcon(device.displayName)}
        title={device.displayName || t('devices.list.labels.unknownDevice')}
        status={{ text: t('devices.memberRemoval.notificationPending.title'), tone: 'info' }}
      />
      <InlineNotice
        tone="info"
        title={t('devices.memberRemoval.notificationPending.title')}
        text={t('devices.memberRemoval.notificationPending.description')}
      />
      <SettingsGroup>
        <DevicesKitFact
          label={t('devices.panel.fields.peerId')}
          value={device.deviceId}
          monospace
          trailing={
            <DevicesKitCopyButton value={device.deviceId} label={t('devices.kit.copyPeerId')} />
          }
        />
      </SettingsGroup>
    </>
  )
}
