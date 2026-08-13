import { createContext, useContext } from 'react'
import type { DeviceTrustChoice, DeviceTrustSnapshot } from '@/api/daemon/device-trust'

export interface DeviceTrustContextValue {
  snapshot: DeviceTrustSnapshot | null
  loading: boolean
  decisionBusy: boolean
  decisionError: string | null
  refresh: () => Promise<void>
  decide: (choice: DeviceTrustChoice, confirmLocalRemoval: boolean) => Promise<void>
}

export const DeviceTrustContext = createContext<DeviceTrustContextValue | null>(null)

export function useDeviceTrust(): DeviceTrustContextValue {
  const value = useContext(DeviceTrustContext)
  if (!value) throw new Error('useDeviceTrust must be used within DeviceTrustProvider')
  return value
}
