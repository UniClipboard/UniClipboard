import { useRef } from 'react'
import type { useDevicesPage } from '@/hooks/useDevicesPage'
import { useSwitchSpace } from '@/hooks/useSwitchSpace'
import type { DevicesKitController } from './useDevicesKit'

/** Navigation and dismissal for the sheet's mutually exclusive task views. */
export function useDevicesKitTasks(
  page: ReturnType<typeof useDevicesPage>,
  devices: DevicesKitController
) {
  const mobileBusy = useRef(false)
  const active = page.switchSpaceOpen
    ? 'join'
    : page.settingsSheetOpen
      ? 'network'
      : page.addDialogOpen
        ? 'phone'
        : null
  const switchSpace = useSwitchSpace({
    autoClose: false,
    onOpenChange: next => {
      if (!next) leaveJoin()
    },
  })
  const leaveJoin = () => {
    page.setSwitchSpaceOpen(false)
    switchSpace.reset()
  }
  const close = () => {
    if (mobileBusy.current || switchSpace.step === 'migrating' || switchSpace.step === 'processing')
      return
    if (active === 'join') leaveJoin()
    else if (active === 'network') {
      page.mobileActions.setSettingsSheetOpen(false)
      page.mobileActions.setAddDialogOpen(true)
    } else if (active === 'phone') page.mobileActions.setAddDialogOpen(false)
    else devices.setOpen(false)
  }
  return {
    active,
    switchSpace,
    close,
    leaveJoin,
    setMobileBusy: (busy: boolean) => {
      mobileBusy.current = busy
    },
    leaveNetwork: () => {
      mobileBusy.current = false
      page.mobileActions.setSettingsSheetOpen(false)
      page.mobileActions.setAddDialogOpen(true)
    },
  }
}
