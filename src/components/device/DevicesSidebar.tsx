import { LayoutGroup, m } from 'framer-motion'
import { RefreshCw, Settings2 } from 'lucide-react'
import type { SpaceMember } from '@/api/daemon/members'
import { type MobileDeviceView } from '@/api/tauri-command/mobile_sync'
import { derivePeerStatusTone } from '@/components/device/connection-channel-utils'
import { getDeviceTrustStatus } from '@/components/device/device-trust-view'
import DeviceListFooter from '@/components/device/DeviceListFooter'
import DeviceListItem from '@/components/device/DeviceListItem'
import SectionLabel from '@/components/device/DeviceSectionLabel'
import EmptyAddRow from '@/components/device/EmptyAddRow'
import LocalDeviceListItem from '@/components/device/LocalDeviceListItem'
import { type StatusDotTone } from '@/components/device/StatusDot'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Skeleton } from '@/components/ui/skeleton'
import { useDevicesPage } from '@/hooks/useDevicesPage'
import { formatRelativeTime } from '@/hooks/useRelativeTime'
import { cn } from '@/lib/utils'
import {
  clearLocalDeviceError,
  clearSpaceMembersError,
  fetchLocalDeviceInfo,
  fetchSpaceMembers,
  fetchSpaceProtection,
} from '@/store/slices/devicesSlice'

const MOBILE_ACTIVE_WINDOW_MS = 10 * 60 * 1000
export default function DevicesSidebar({ page }: { page: ReturnType<typeof useDevicesPage> }) {
  const {
    selectionId,
    t,
    dispatch,
    now,
    localDevice,
    localDeviceError,
    spaceMembersError,
    spaceProtectionError,
    networkRecovery,
    networkRecoveryError,
    manualRefreshInProgress,
    peers,
    onlineCount,
    localDeviceStatus,
    refreshConnectionsManually,
    trustListView,
    mobileDevices,
    mobileDevicesError,
    mobileActions,
    setSelection,
    effectiveSelection,
    setSwitchSpaceOpen,
    setAddP2PDialogOpen,
    networkRecoveryVisible,
    canRetryNetworkRecovery,
    requestNetworkRecoveryNow,
  } = page
  return (
    <aside className="relative flex w-56 shrink-0 flex-col border-r border-border/50 bg-muted/15 xl:w-64">
      <div className="px-3 pt-3">
        {networkRecoveryVisible && (
          <Alert className="mt-2 border-warning/30 bg-warning/10 text-warning">
            <AlertDescription className="flex flex-col gap-2 text-ui-caption">
              <span>
                {networkRecoveryError
                  ? t(networkRecoveryError)
                  : t(`devices.networkRecovery.${networkRecovery?.phase}`)}
              </span>
              {canRetryNetworkRecovery && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="self-start"
                  onClick={requestNetworkRecoveryNow}
                >
                  <RefreshCw />
                  {t('devices.networkRecovery.retry')}
                </Button>
              )}
            </AlertDescription>
          </Alert>
        )}
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <LayoutGroup id={selectionId}>
          <m.nav
            layoutRoot
            aria-label={t('devices.panel.listTitle')}
            data-device-list
            className="flex flex-col gap-1 px-2 pb-3"
          >
            {(spaceMembersError || mobileDevicesError || spaceProtectionError || false) && (
              <Alert variant="destructive" className="mx-1 my-2">
                <AlertDescription className="flex flex-col gap-2 text-ui-caption">
                  <span>
                    {spaceMembersError ??
                      mobileDevicesError ??
                      (spaceProtectionError ? t(spaceProtectionError) : null)}
                  </span>
                  <Button
                    variant="outline"
                    size="sm"
                    className="self-start"
                    onClick={() => {
                      if (spaceMembersError) {
                        dispatch(clearSpaceMembersError())
                        dispatch(fetchSpaceMembers())
                      }
                      if (spaceProtectionError) {
                        dispatch(fetchSpaceProtection())
                      }
                      if (mobileDevicesError) {
                        mobileActions.reload()
                      }
                    }}
                  >
                    {t('devices.list.actions.retry')}
                  </Button>
                </AlertDescription>
              </Alert>
            )}

            <SectionLabel label={t('devices.thisDevice.title')} />
            {localDevice ? (
              <LocalDeviceListItem
                name={localDevice.deviceName}
                status={localDeviceStatus}
                selected={effectiveSelection.kind === 'local'}
                onSelect={() => setSelection({ kind: 'local' })}
              />
            ) : (
              <div className="px-2.5 py-2">
                {localDeviceError ? (
                  <button
                    type="button"
                    className="text-left text-ui-body text-destructive underline underline-offset-2"
                    onClick={() => {
                      dispatch(clearLocalDeviceError())
                      dispatch(fetchLocalDeviceInfo())
                    }}
                  >
                    {t('devices.list.actions.retry')}
                  </button>
                ) : (
                  <Skeleton className="h-5 w-32" />
                )}
              </div>
            )}

            <SectionLabel label={t('devices.pairedDevices.title')}>
              <button
                type="button"
                aria-label={t('devices.connectionRefresh.action')}
                title={t(
                  manualRefreshInProgress
                    ? 'devices.connectionRefresh.checking'
                    : 'devices.connectionRefresh.action'
                )}
                disabled={manualRefreshInProgress}
                className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground disabled:cursor-wait disabled:opacity-60"
                onClick={() => void refreshConnectionsManually()}
              >
                <RefreshCw
                  aria-hidden="true"
                  className={cn('size-3.5', manualRefreshInProgress && 'motion-safe:animate-spin')}
                />
              </button>
            </SectionLabel>
            {peers.map(peer => {
              const trust = trustListView.relationshipsByDeviceId.get(peer.peerId)
              const trustStatus = trust ? getDeviceTrustStatus(trust, t) : null
              return (
                <DeviceListItem
                  key={peer.peerId}
                  testId={`device-peer-${peer.peerId}`}
                  name={peer.deviceName || t('devices.list.labels.unknownDevice')}
                  tone={trustStatus?.tone ?? peerDotTone(peer)}
                  status={
                    trustStatus?.status ?? {
                      kind: peer.connected ? 'online' : 'offline',
                      label: t(`devices.list.status.${peer.connected ? 'online' : 'offline'}`),
                    }
                  }
                  dimmed={!peer.connected && !trustStatus}
                  selected={
                    effectiveSelection.kind === 'peer' && effectiveSelection.id === peer.peerId
                  }
                  onSelect={() => setSelection({ kind: 'peer', id: peer.peerId })}
                />
              )
            })}
            {peers.length === 0 && !spaceMembersError && (
              <EmptyAddRow
                label={t('devices.panel.addMenu.trigger')}
                onClick={() => setAddP2PDialogOpen(true)}
              />
            )}

            {mobileDevices.length > 0 && (
              <>
                <SectionLabel label={t('devices.mobileSync.title')}>
                  <button
                    type="button"
                    aria-label={t('devices.mobileSync.configure')}
                    title={t('devices.mobileSync.configure')}
                    className="rounded-md p-0.5 text-muted-foreground/70 transition-colors hover:text-foreground"
                    onClick={mobileActions.openSettings}
                  >
                    <Settings2 className="size-3.5" />
                  </button>
                </SectionLabel>
                {mobileDevices.map(mobile => {
                  const tone = mobileDotTone(mobile, now)
                  return (
                    <DeviceListItem
                      key={mobile.deviceId}
                      name={mobile.label}
                      tone={tone}
                      status={{
                        kind: 'recently_active',
                        label:
                          mobile.lastSeenAtMs == null
                            ? t('devices.mobileSync.list.lastSeen.never')
                            : formatRelativeTime(mobile.lastSeenAtMs, now, t),
                      }}
                      dimmed={tone === 'off'}
                      selected={
                        effectiveSelection.kind === 'mobile' &&
                        effectiveSelection.id === mobile.deviceId
                      }
                      onSelect={() =>
                        setSelection(current => ({
                          kind: 'mobile',
                          id: mobile.deviceId,
                          pendingCredential:
                            current.kind === 'mobile' && current.id === mobile.deviceId
                              ? current.pendingCredential
                              : undefined,
                        }))
                      }
                    />
                  )
                })}
              </>
            )}
          </m.nav>
        </LayoutGroup>
      </ScrollArea>
      <DeviceListFooter
        onlineCount={onlineCount}
        onAddDevice={() => setAddP2PDialogOpen(true)}
        onSwitchSpace={() => setSwitchSpaceOpen(true)}
        onAddMobile={mobileActions.handleAddClick}
      />
    </aside>
  )
}
function peerDotTone(peer: SpaceMember): StatusDotTone {
  return derivePeerStatusTone(peer.channel ?? 'unknown', peer.connected)
}

function mobileDotTone(mobile: MobileDeviceView, now: number): StatusDotTone {
  if (mobile.lastSeenAtMs == null) return 'off'
  return now - mobile.lastSeenAtMs <= MOBILE_ACTIVE_WINDOW_MS ? 'info' : 'off'
}
