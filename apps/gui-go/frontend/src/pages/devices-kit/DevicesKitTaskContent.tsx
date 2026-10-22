import type { RegisterMobileDeviceResult } from '@/api/tauri-command/mobile_sync'
import type { useDevicesPage } from '@/hooks/useDevicesPage'
import { DevicesKitConnectPhone } from './DevicesKitConnectPhone'
import { DevicesKitPhoneComplete } from './DevicesKitPhoneComplete'
import { DevicesKitPhoneNetwork } from './DevicesKitPhoneNetwork'
import { DevicesKitSwitchSpace } from './DevicesKitSwitchSpace'
import type { DevicesKitController } from './useDevicesKit'
import type { useDevicesKitTasks } from './useDevicesKitTasks'

export function DevicesKitTaskContent({
  tasks,
  page,
  devices,
  onCredential,
  credential,
  onCredentialDone,
}: {
  tasks: ReturnType<typeof useDevicesKitTasks>
  page: ReturnType<typeof useDevicesPage>
  devices: DevicesKitController
  onCredential: (result: RegisterMobileDeviceResult) => void
  credential: RegisterMobileDeviceResult | null
  onCredentialDone: () => void
}) {
  if (credential && tasks.active === 'phone')
    return <DevicesKitPhoneComplete credential={credential} onDone={onCredentialDone} />
  if (tasks.active === 'join')
    return <DevicesKitSwitchSpace model={tasks.switchSpace} onBack={tasks.leaveJoin} />
  if (tasks.active === 'network')
    return (
      <DevicesKitPhoneNetwork
        onBusyChange={tasks.setMobileBusy}
        onBack={tasks.leaveNetwork}
        onSettingsChange={page.mobileActions.setSettings}
      />
    )
  return (
    <DevicesKitConnectPhone
      onBusyChange={tasks.setMobileBusy}
      onBack={() => page.mobileActions.setAddDialogOpen(false)}
      onDirect={() => {
        page.mobileActions.setAddDialogOpen(false)
        devices.invite()
      }}
      onSettingsChange={page.mobileActions.setSettings}
      onConfigure={() => {
        page.mobileActions.setAddDialogOpen(false)
        page.mobileActions.openSettings()
      }}
      onSuccess={result => {
        tasks.setMobileBusy(false)
        page.mobileActions.reload()
        onCredential(result)
        devices.setSelection({ kind: 'mobile', id: result.deviceId })
      }}
    />
  )
}
