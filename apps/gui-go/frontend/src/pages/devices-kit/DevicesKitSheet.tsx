import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import ConnectMobileDialog from '@/components/device/ConnectMobileDialog'
import MobileSyncSettingsDialog from '@/components/device/MobileSyncSettingsDialog'
import SwitchSpaceDialog from '@/components/device/SwitchSpaceDialog'
import { useDevicesPage } from '@/hooks/useDevicesPage'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import { fetchSpaceMembers, fetchSpaceProtection } from '@/store/slices/devicesSlice'
import { AlertDialog, Button, MasterDetail, Menu, Sheet, type MenuEntry } from '@/ui-kit'
import { selectionId, type DevicesKitSelection } from './devices-kit-model'
import { DevicesKitDetail } from './DevicesKitDetail'
import { DevicesKitList } from './DevicesKitList'
import type { DevicesKitController } from './useDevicesKit'

const INVITE = 'invite'
const PHONE = 'phone'
const JOIN = 'join'

/**
 * Manage Devices (design DV1–DV7): a sheet over the History window, opened from the gear on
 * the sidebar's Devices heading, the same pattern as Manage Tags. The device list and the detail
 * of the selected row inside; + (Invite Device, Connect Phone, Join Another Space), − (unpair or
 * revoke the selected device) and Done in the footer. Mounted from opening until it has animated
 * out, so the device page's polling and subscriptions run only then.
 */
export function DevicesKitSheet({ devices }: { devices: DevicesKitController }) {
  const { t } = useTranslation()
  const page = useDevicesPage()
  const { invitation } = devices
  // Registration credentials belong to this Sheet session, never to History's saved selection.
  const [pendingCredential, setPendingCredential] = useState<RegisterMobileDeviceResult | null>(
    null
  )
  const select = (selection: DevicesKitSelection) => {
    setPendingCredential(null)
    devices.setSelection(selection)
  }
  // The sheet mounts closed and opens after its first commit: a dialog that mounts already open
  // skips its entry animation, and this one is mounted only while in use.
  const [entered, setEntered] = useState(false)
  useEffect(() => setEntered(true), [])
  useShortcutLayer({ layer: 'modal', scope: 'modal', enabled: devices.open })

  // A row that is gone (unpaired, revoked, not loaded yet) shows this device for now; the stored
  // selection is kept, so a device that is still loading is selected once it arrives.
  const wanted = devices.selection
  const resolved: DevicesKitSelection =
    (wanted.kind === 'peer' && !page.peers.some(p => p.peerId === wanted.id)) ||
    (wanted.kind === 'removed' &&
      !page.trustListView.removedDevices.some(d => d.deviceId === wanted.id)) ||
    (wanted.kind === 'mobile' && !page.mobileDevices.some(d => d.deviceId === wanted.id)) ||
    (wanted.kind === 'invitation' && !invitation.active)
      ? { kind: 'local' }
      : wanted

  const selectedMobile =
    resolved.kind === 'mobile'
      ? page.mobileDevices.find(d => d.deviceId === resolved.id)
      : undefined
  const removable = resolved.kind === 'peer' || selectedMobile !== undefined

  const addMenu: MenuEntry[] = [
    {
      id: INVITE,
      label: t(invitation.active ? 'devices.kit.showInvitation' : 'devices.kit.inviteDevice'),
      icon: 'add',
    },
    { id: PHONE, label: t('devices.kit.connectPhone'), icon: 'phone' },
    { id: 'separator', kind: 'separator' },
    { id: JOIN, label: t('devices.kit.joinSpace') },
  ]
  const refreshMembers = () => {
    page.dispatch(fetchSpaceMembers())
    page.dispatch(fetchSpaceProtection())
  }

  return (
    <Sheet
      open={devices.open && entered}
      onOpenChange={devices.setOpen}
      onClosed={devices.onClosed}
      title={t('devices.panel.listTitle')}
      footer={
        <>
          <Menu
            label={t('devices.kit.add')}
            icon="add"
            iconOnly
            items={addMenu}
            onAction={id => {
              if (id === INVITE) devices.invite()
              else if (id === PHONE) page.mobileActions.handleAddClick()
              else if (id === JOIN) page.setSwitchSpaceOpen(true)
            }}
          />
          <Button
            icon="remove"
            label={t('devices.kit.remove')}
            disabled={!removable}
            onClick={() => {
              if (resolved.kind === 'peer') page.handleUnpairRequest(resolved.id)
              else if (selectedMobile) page.mobileActions.requestRevoke(selectedMobile)
            }}
          />
        </>
      }
      done={{ label: t('history.composite.done'), onClick: () => devices.setOpen(false) }}
    >
      <MasterDetail
        detailKey={selectionId(resolved)}
        list={
          <DevicesKitList
            page={page}
            invitation={invitation}
            selection={resolved}
            onSelect={select}
          />
        }
        detailLabel={t('devices.kit.detailLabel')}
        detail={
          <DevicesKitDetail
            page={page}
            selection={resolved}
            invitation={invitation}
            initialCredential={pendingCredential}
            onCredentialConsumed={() => setPendingCredential(null)}
            joinedPeerId={devices.joinedPeerId}
            onDismissJoined={devices.dismissJoined}
          />
        }
      />

      <AlertDialog
        open={page.unpairDialogOpen}
        onOpenChange={page.handleUnpairDialogOpenChange}
        title={t('devices.unpair.confirmTitle')}
        description={t('devices.unpair.confirmDescription', {
          deviceName: page.unpairTargetDevice?.deviceName || t('devices.list.labels.unknownDevice'),
        })}
        confirmLabel={t('devices.list.actions.unpair')}
        cancelLabel={t('clipboard.cancelLabel')}
        destructive
        busyLabel={page.unpairBusy ? t('devices.unpair.cancelling') : undefined}
        onConfirm={page.handleUnpairConfirm}
      />
      <AlertDialog
        open={page.revokeTarget !== null}
        onOpenChange={open => {
          if (!open && !page.revokeBusy) page.mobileActions.clearRevokeTarget()
        }}
        title={t('devices.mobileSync.revoke.confirmTitle', {
          label: page.revokeTarget?.label ?? '',
        })}
        description={t('devices.mobileSync.revoke.confirmDescription')}
        confirmLabel={t('devices.mobileSync.revoke.confirm')}
        cancelLabel={t('devices.mobileSync.revoke.cancel')}
        destructive
        busyLabel={page.revokeBusy ? t('devices.mobileSync.revoke.confirm') : undefined}
        onConfirm={page.mobileActions.handleRevokeConfirm}
      />

      {/* The app's own dialogs for these flows, nested in the sheet so focus moves to them. */}
      <SwitchSpaceDialog open={page.switchSpaceOpen} onOpenChange={page.setSwitchSpaceOpen} />
      <MobileSyncSettingsDialog
        open={page.settingsSheetOpen}
        onOpenChange={page.mobileActions.setSettingsSheetOpen}
        onSettingsChange={page.mobileActions.setSettings}
      />
      <ConnectMobileDialog
        open={page.addDialogOpen}
        onOpenChange={page.mobileActions.setAddDialogOpen}
        onSettingsChange={page.mobileActions.setSettings}
        onConfigure={() => {
          page.mobileActions.setAddDialogOpen(false)
          page.mobileActions.openSettings()
        }}
        onDirectSuccess={refreshMembers}
        onMobileSuccess={result => {
          page.mobileActions.reload()
          setPendingCredential(result)
          devices.setSelection({ kind: 'mobile', id: result.deviceId })
        }}
      />
    </Sheet>
  )
}
