import { useTranslation } from 'react-i18next'
import { derivePeerStatusTone } from '@/components/device/connection-channel-utils'
import { getDeviceTrustStatus } from '@/components/device/device-trust-view'
import type { useDevicesPage } from '@/hooks/useDevicesPage'
import { formatRelativeTime } from '@/hooks/useRelativeTime'
import { useSettingSelector } from '@/hooks/useSetting'
import { isMobileDeviceActive } from '@/lib/mobile-device-status'
import {
  clearLocalDeviceError,
  clearSpaceMembersError,
  fetchLocalDeviceInfo,
  fetchSpaceMembers,
  fetchSpaceProtection,
} from '@/store/slices/devicesSlice'
import {
  InlineNotice,
  InsetList,
  SourceList,
  type SourceListItem,
  type SourceListSection,
} from '@/ui-kit'
import {
  deviceIcon,
  dotTone,
  INVITATION_ROW_ID,
  invitationSummary,
  parseSelectionId,
  type DevicesKitSelection,
  selectionId,
} from './devices-kit-model'
import { DevicesKitSpaceUpdate } from './DevicesKitSpaceUpdate'
import type { DeviceInvitationRow } from './useDeviceInvitationRow'

/**
 * The device list of Manage Devices (design DV1–DV3, DV7): This Device, Paired devices with the
 * invitation row on top and Refresh in the heading, Removed devices, the read-only Pairing history
 * and LAN Mobile Sync with its Configure command. Rows show presence or the trust label; the dot
 * shows the channel (green direct, blue relayed, grey disconnected).
 */
export function DevicesKitList({
  page,
  invitation,
  selection,
  onSelect,
}: {
  page: ReturnType<typeof useDevicesPage>
  invitation: DeviceInvitationRow
  selection: DevicesKitSelection
  onSelect: (selection: DevicesKitSelection) => void
}) {
  const { t } = useTranslation()
  const syncActive = useSettingSelector(
    ({ setting }) => setting !== null && setting.sync.syncEnabled !== false
  )
  const {
    dispatch,
    now,
    localDevice,
    localDeviceError,
    localDeviceStatus,
    peers,
    trustListView,
    deviceTrust,
    deviceTrustError,
    deviceTrustFailure,
    refreshDeviceTrust,
    mobileDevices,
    mobileActions,
    spaceMembersError,
    mobileDevicesError,
    spaceProtectionError,
    manualRefreshInProgress,
    refreshConnectionsManually,
    networkRecovery,
    networkRecoveryError,
    networkRecoveryVisible,
    canRetryNetworkRecovery,
    requestNetworkRecoveryNow,
    onlineCount,
  } = page

  const localNeedsAttention =
    localDeviceStatus != null && !['online', 'paused', 'unknown'].includes(localDeviceStatus.kind)
  const localItems: SourceListItem[] = localDevice
    ? [
        {
          id: 'local',
          title: localDevice.deviceName,
          subtitle:
            localDeviceStatus?.label ??
            t(syncActive ? 'devices.thisDevice.syncActive' : 'devices.thisDevice.syncPaused'),
          subtitleTone: localNeedsAttention ? 'warning' : undefined,
          icon: deviceIcon(localDevice.deviceName),
          status: localNeedsAttention ? 'warning' : syncActive ? 'positive' : 'neutral',
        },
      ]
    : []

  const invitationItems: SourceListItem[] = invitation.active
    ? [
        {
          'data-testid': 'device-invitation-row',
          id: INVITATION_ROW_ID,
          title: t('devices.kit.invitation'),
          ...(() => {
            const summary = invitationSummary(invitation.state, t)
            return {
              subtitle: summary.text,
              status: summary.tone,
              subtitleTone: summary.tone === 'critical' ? ('critical' as const) : undefined,
            }
          })(),
          icon: 'add',
          pending: true,
        },
      ]
    : []

  const peerItems = peers.map((peer): SourceListItem => {
    const trust = trustListView.relationshipsByDeviceId.get(peer.peerId)
    const trustStatus = trust ? getDeviceTrustStatus(trust, t) : null
    return {
      'data-testid': `device-peer-${peer.peerId}`,
      'data-status': trustStatus?.status.kind ?? (peer.connected ? 'online' : 'offline'),
      id: selectionId({ kind: 'peer', id: peer.peerId }),
      title: peer.deviceName || t('devices.list.labels.unknownDevice'),
      subtitle:
        trustStatus?.status.label ??
        t(peer.connected ? 'devices.list.status.online' : 'devices.list.status.offline'),
      subtitleTone: trustStatus?.tone === 'warning' ? 'warning' : undefined,
      icon: deviceIcon(peer.deviceName),
      status:
        dotTone[
          trustStatus?.tone ?? derivePeerStatusTone(peer.channel ?? 'unknown', peer.connected)
        ],
      dimmed: !peer.connected && !trustStatus,
    }
  })

  const removedItems = trustListView.removedDevices.flatMap((device): SourceListItem[] => {
    const trustStatus = getDeviceTrustStatus(device, t)
    if (!trustStatus) return []
    return [
      {
        'data-testid': `removed-device-${device.deviceId}`,
        'data-status': trustStatus.status.kind,
        id: selectionId({ kind: 'removed', id: device.deviceId }),
        title: device.displayName || t('devices.list.labels.unknownDevice'),
        subtitle: trustStatus.status.label,
        icon: deviceIcon(device.displayName),
        status: dotTone[trustStatus.tone],
        dimmed: true,
      },
    ]
  })

  const mobileItems = mobileDevices.map((mobile): SourceListItem => {
    const active = isMobileDeviceActive(mobile, now)
    return {
      id: selectionId({ kind: 'mobile', id: mobile.deviceId }),
      title: mobile.label,
      subtitle:
        mobile.lastSeenAtMs == null
          ? t('devices.mobileSync.list.lastSeen.never')
          : formatRelativeTime(mobile.lastSeenAtMs, now, t),
      icon: 'phone',
      status: active ? 'info' : 'neutral',
      dimmed: !active,
    }
  })

  const pairings = deviceTrust?.inboundPairings ?? []
  const sections: SourceListSection[] = [
    { id: 'local', title: t('devices.thisDevice.title'), items: localItems },
    {
      id: 'paired',
      title: t('devices.pairedDevices.title'),
      items: [...invitationItems, ...peerItems],
      action: {
        label: t(
          manualRefreshInProgress
            ? 'devices.connectionRefresh.checking'
            : 'devices.connectionRefresh.action'
        ),
        icon: 'retry',
        busy: manualRefreshInProgress,
        onClick: () => void refreshConnectionsManually(),
      },
      placeholder: spaceMembersError
        ? undefined
        : { label: t('devices.list.actions.addDevice'), onClick: invitation.start },
      extra: (
        <DevicesKitSpaceUpdate
          status={deviceTrust?.spaceDeviceUpdate}
          loadFailure={
            !deviceTrust && deviceTrustError ? (deviceTrustFailure ?? 'unavailable') : null
          }
          onRetry={() => void refreshDeviceTrust()}
        />
      ),
    },
    ...(removedItems.length > 0
      ? [{ id: 'removed', title: t('devices.memberRemoval.removedDevices'), items: removedItems }]
      : []),
    ...(pairings.length > 0
      ? [
          {
            id: 'pairings',
            title: t('devices.inboundPairings.title'),
            items: [],
            extra: (
              <InsetList
                label={t('devices.inboundPairings.title')}
                rows={pairings.map(pairing => ({
                  id: pairing.pairingId,
                  title: pairing.displayName || t('devices.inboundPairings.unnamed'),
                  detail: t(`devices.inboundPairings.status.${pairing.status}`),
                  tone: pairing.status === 'needs_attention' ? 'warning' : 'neutral',
                }))}
              />
            ),
          },
        ]
      : []),
    ...(mobileItems.length > 0
      ? [
          {
            id: 'mobile',
            title: t('devices.mobileSync.title'),
            items: mobileItems,
            action: {
              label: t('devices.mobileSync.configure'),
              icon: 'settings' as const,
              onClick: mobileActions.openSettings,
            },
          },
        ]
      : []),
  ]

  const listError =
    spaceMembersError ??
    mobileDevicesError ??
    (spaceProtectionError ? t(spaceProtectionError) : null) ??
    (localDeviceError && !localDevice ? localDeviceError : null)

  return (
    <SourceList
      label={t('devices.panel.listTitle')}
      sections={sections}
      value={selectionId(selection)}
      onValueChange={id => onSelect(parseSelectionId(id))}
      header={
        <>
          {networkRecoveryVisible && (
            <InlineNotice
              tone="warning"
              title={
                networkRecoveryError
                  ? t(networkRecoveryError)
                  : t(`devices.networkRecovery.${networkRecovery?.phase}`)
              }
              action={
                canRetryNetworkRecovery
                  ? {
                      label: t('devices.networkRecovery.retry'),
                      onClick: requestNetworkRecoveryNow,
                    }
                  : undefined
              }
            />
          )}
          {listError && (
            <InlineNotice
              tone="error"
              title={listError}
              action={{
                label: t('devices.list.actions.retry'),
                onClick: () => {
                  if (spaceMembersError) {
                    dispatch(clearSpaceMembersError())
                    dispatch(fetchSpaceMembers())
                  }
                  if (spaceProtectionError) dispatch(fetchSpaceProtection())
                  if (mobileDevicesError) mobileActions.reload()
                  if (localDeviceError) {
                    dispatch(clearLocalDeviceError())
                    dispatch(fetchLocalDeviceInfo())
                  }
                },
              }}
            />
          )}
        </>
      }
      footer={t('devices.thisDevice.onlineCount', { count: onlineCount })}
    />
  )
}
