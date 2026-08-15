import { createContext } from 'react'
import type { DeviceTrustChoice, DeviceTrustSnapshot } from '@/api/daemon/device-trust'

export interface DeviceTrustContextValue {
  snapshot: DeviceTrustSnapshot | null
  loading: boolean
  decisionBusy: boolean
  decisionError: string | null
  localRemovalConfirmationChangeId: string | null
  refresh: () => Promise<void>
  decide: (choice: DeviceTrustChoice, confirmLocalRemoval: boolean) => Promise<void>
}

export const DeviceTrustContext = createContext<DeviceTrustContextValue | null>(null)
