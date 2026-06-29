import { useMemo } from 'react'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import { useAppSelector } from '@/store/hooks'

export function useHistorySourceOptions() {
  const spaceMembers = useAppSelector(s => s.devices.spaceMembers)
  const mobileDevices = useMobileDeviceList()

  return useMemo(
    () => [
      ...spaceMembers.map(m => ({ id: m.peerId, name: m.deviceName, kind: 'p2p' as const })),
      ...mobileDevices.map(d => ({
        id: `mobile_sync:${d.deviceId}`,
        name: d.label,
        kind: 'mobile' as const,
      })),
    ],
    [spaceMembers, mobileDevices]
  )
}
