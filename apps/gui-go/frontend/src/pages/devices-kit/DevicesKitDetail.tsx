import { useTranslation } from 'react-i18next'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import MobileDevicePanel from '@/components/device/MobileDevicePanel'
import type { useDevicesPage } from '@/hooks/useDevicesPage'
import { createLogger } from '@/lib/logger'
import {
  clearLocalDeviceError,
  fetchLocalDeviceInfo,
  fetchNetworkRecoveryStatus,
  fetchSpaceMembers,
  fetchSpaceProtection,
} from '@/store/slices/devicesSlice'
import { ContentUnavailable, InlineNotice } from '@/ui-kit'
import type { DevicesKitSelection } from './devices-kit-model'
import { DevicesKitInvitationDetail } from './DevicesKitInvitationDetail'
import { DevicesKitJoinedNotice } from './DevicesKitJoinedNotice'
import { DevicesKitLocalDetail } from './DevicesKitLocalDetail'
import { DevicesKitPeerDetail } from './DevicesKitPeerDetail'
import { DevicesKitRemovedDetail } from './DevicesKitRemovedDetail'
import type { DeviceInvitationRow } from './useDeviceInvitationRow'

const log = createLogger('devices-kit')

/** The detail column of Manage Devices for the selected row. */
export function DevicesKitDetail({
  page,
  selection,
  invitation,
  initialCredential,
  onCredentialConsumed,
  joinedPeerId,
  onDismissJoined,
}: {
  page: ReturnType<typeof useDevicesPage>
  /** Already resolved: it names a row that exists. */
  selection: DevicesKitSelection
  invitation: DeviceInvitationRow
  initialCredential: RegisterMobileDeviceResult | null
  onCredentialConsumed: () => void
  joinedPeerId: string | null
  onDismissJoined: () => void
}) {
  const { t } = useTranslation()
  const { dispatch } = page

  switch (selection.kind) {
    case 'invitation':
      return <DevicesKitInvitationDetail row={invitation} />
    case 'peer': {
      const device = page.peers.find(peer => peer.peerId === selection.id)
      if (!device) return null
      return (
        <>
          {joinedPeerId === device.peerId && <DevicesKitJoinedNotice onDismiss={onDismissJoined} />}
          <DevicesKitPeerDetail
            key={device.peerId}
            device={device}
            status={page.getDeviceTrustStatusForPeer(device.peerId)}
            onUnpair={() => page.handleUnpairRequest(device.peerId)}
          />
        </>
      )
    }
    case 'removed': {
      const device = page.trustListView.removedDevices.find(d => d.deviceId === selection.id)
      return device ? <DevicesKitRemovedDetail device={device} /> : null
    }
    case 'mobile': {
      const device = page.mobileDevices.find(d => d.deviceId === selection.id)
      if (!device) return null
      // The phone panel (pairing QR, credentials, edit) is still the app's own panel.
      return (
        <MobileDevicePanel
          key={device.deviceId}
          device={device}
          settings={page.mobileSettings}
          initialCredential={
            initialCredential?.deviceId === device.deviceId ? initialCredential : null
          }
          onRevoke={page.mobileActions.requestRevoke}
          onInitialCredentialConsumed={onCredentialConsumed}
          onRotated={page.mobileActions.reload}
        />
      )
    }
    case 'local':
      if (page.localDevice) {
        return (
          <DevicesKitLocalDetail
            localDevice={page.localDevice}
            memberCount={page.peers.length + 1}
            status={page.localDeviceStatus}
            onRebuildSucceeded={() => {
              dispatch(fetchSpaceMembers())
              dispatch(fetchSpaceProtection())
              dispatch(fetchNetworkRecoveryStatus())
              void page.refreshDeviceTrust().catch(error => {
                log.warn({ err: error }, 'Device trust refresh failed after space rebuild')
              })
            }}
          />
        )
      }
      if (page.localDeviceError) {
        return (
          <InlineNotice
            tone="error"
            title={page.localDeviceError}
            action={{
              label: t('devices.list.actions.retry'),
              onClick: () => {
                dispatch(clearLocalDeviceError())
                dispatch(fetchLocalDeviceInfo())
              },
            }}
          />
        )
      }
      return <ContentUnavailable title={t('common.loading')} />
  }
}
