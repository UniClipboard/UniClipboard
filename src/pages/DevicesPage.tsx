import React from 'react'
import AddDeviceDialog from '@/components/device/AddDeviceDialog'
import ConnectMobileDialog from '@/components/device/ConnectMobileDialog'
import DevicesSidebar from '@/components/device/DevicesSidebar'
import LocalDevicePanel from '@/components/device/LocalDevicePanel'
import LocalPanelSkeleton from '@/components/device/LocalPanelSkeleton'
import MobileDevicePanel from '@/components/device/MobileDevicePanel'
import MobileSyncSettingsDialog from '@/components/device/MobileSyncSettingsDialog'
import PeerDetailPanelContainer from '@/components/device/PeerDetailPanelContainer'
import SwitchSpaceDialog from '@/components/device/SwitchSpaceDialog'
import UnpairAlertDialog from '@/components/device/UnpairAlertDialog'
import { Alert, AlertDescription } from '@/components/ui/alert'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useDevicesPage } from '@/hooks/useDevicesPage'
import { createLogger } from '@/lib/logger'
import {
  clearLocalDeviceError,
  fetchLocalDeviceInfo,
  fetchNetworkRecoveryStatus,
  fetchSpaceMembers,
  fetchSpaceProtection,
} from '@/store/slices/devicesSlice'

const log = createLogger('devices-page')
const DevicesPage: React.FC = () => {
  const page = useDevicesPage()
  const {
    t,
    dispatch,
    localDevice,
    localDeviceLoading,
    localDeviceError,
    peers,
    localDeviceStatus,
    refreshDeviceTrust,
    mobileSettings,
    addDialogOpen,
    settingsSheetOpen,
    revokeTarget,
    revokeBusy,
    mobileActions,
    selection,
    setSelection,
    selectedPeer,
    selectedMobile,
    effectiveSelection,
    switchSpaceOpen,
    setSwitchSpaceOpen,
    addP2PDialogOpen,
    setAddP2PDialogOpen,
    unpairDialogOpen,
    unpairBusy,
    handleUnpairRequest,
    handleUnpairDialogOpenChange,
    handleUnpairConfirm,
    getDeviceTrustStatusForPeer,
    unpairTargetDevice,
  } = page
  return (
    <div className="flex h-full min-w-0">
      {/* ── list column ───────────────────────────────────────── */}
      <DevicesSidebar page={page} />

      {/* ── detail pane ───────────────────────────────────────── */}
      <main className="min-w-0 flex-1 bg-muted/20">
        <ScrollArea
          key={
            effectiveSelection.kind === 'local'
              ? 'local'
              : `${effectiveSelection.kind}:${effectiveSelection.id}`
          }
          className="h-full [&_[data-slot=scroll-area-viewport]>div]:min-h-full [&_[data-slot=scroll-area-viewport]>div]:!block"
        >
          {effectiveSelection.kind === 'local' &&
            (localDevice ? (
              <LocalDevicePanel
                localDevice={localDevice}
                memberCount={peers.length + 1}
                status={localDeviceStatus}
                onRebuildSucceeded={() => {
                  dispatch(fetchSpaceMembers())
                  dispatch(fetchSpaceProtection())
                  dispatch(fetchNetworkRecoveryStatus())
                  void refreshDeviceTrust().catch(error => {
                    log.warn({ err: error }, 'Device trust refresh failed after space rebuild')
                  })
                }}
              />
            ) : localDeviceError ? (
              <div className="mx-auto w-full max-w-2xl px-8 py-8">
                <Alert variant="destructive">
                  <AlertDescription className="flex items-center gap-3">
                    <span className="flex-1">{localDeviceError}</span>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        dispatch(clearLocalDeviceError())
                        dispatch(fetchLocalDeviceInfo())
                      }}
                    >
                      {t('devices.list.actions.retry')}
                    </Button>
                  </AlertDescription>
                </Alert>
              </div>
            ) : (
              localDeviceLoading && <LocalPanelSkeleton />
            ))}

          {effectiveSelection.kind === 'peer' && selectedPeer && (
            <PeerDetailPanelContainer
              key={selectedPeer.peerId}
              deviceId={selectedPeer.peerId}
              device={selectedPeer}
              status={getDeviceTrustStatusForPeer(selectedPeer.peerId)}
              onUnpair={handleUnpairRequest}
            />
          )}

          {effectiveSelection.kind === 'mobile' && selectedMobile && (
            <MobileDevicePanel
              key={selectedMobile.deviceId}
              device={selectedMobile}
              settings={mobileSettings}
              initialCredential={
                selection.kind === 'mobile' && selection.id === selectedMobile.deviceId
                  ? (selection.pendingCredential ?? null)
                  : null
              }
              onRevoke={mobileActions.requestRevoke}
              onRotated={mobileActions.reload}
            />
          )}
        </ScrollArea>
      </main>

      {/* ── flow dialogs ──────────────────────────────────────── */}
      <SwitchSpaceDialog open={switchSpaceOpen} onOpenChange={setSwitchSpaceOpen} />
      <AddDeviceDialog
        open={addP2PDialogOpen}
        onOpenChange={setAddP2PDialogOpen}
        onSuccess={() => {
          dispatch(fetchSpaceMembers())
          dispatch(fetchSpaceProtection())
        }}
      />
      <UnpairAlertDialog
        open={unpairDialogOpen}
        onOpenChange={handleUnpairDialogOpenChange}
        deviceName={unpairTargetDevice?.deviceName || t('devices.list.labels.unknownDevice')}
        busy={unpairBusy}
        onConfirm={handleUnpairConfirm}
      />
      <MobileSyncSettingsDialog
        open={settingsSheetOpen}
        onOpenChange={mobileActions.setSettingsSheetOpen}
        onSettingsChange={mobileActions.setSettings}
      />
      <ConnectMobileDialog
        open={addDialogOpen}
        onOpenChange={mobileActions.setAddDialogOpen}
        onSettingsChange={mobileActions.setSettings}
        onConfigure={() => {
          mobileActions.setAddDialogOpen(false)
          mobileActions.openSettings()
        }}
        onDirectSuccess={() => {
          dispatch(fetchSpaceMembers())
          dispatch(fetchSpaceProtection())
        }}
        onMobileSuccess={result => {
          // Retire the credential modal: refresh the list, select the new
          // device, and hand its one-time credentials to the panel's fresh
          // state (pairing QR + credentials + install helper) inline.
          mobileActions.reload()
          setSelection({
            kind: 'mobile',
            id: result.deviceId,
            pendingCredential: result,
          })
        }}
      />

      <AlertDialog
        open={!!revokeTarget}
        onOpenChange={open => {
          if (!open && !revokeBusy) mobileActions.clearRevokeTarget()
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t('devices.mobileSync.revoke.confirmTitle', {
                label: revokeTarget?.label ?? '',
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.mobileSync.revoke.confirmDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={revokeBusy}>
              {t('devices.mobileSync.revoke.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={mobileActions.handleRevokeConfirm}
              disabled={revokeBusy}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              {t('devices.mobileSync.revoke.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

export default React.memo(DevicesPage)
