import { createContext } from 'react'
import type { DeviceGroupChoices, DeviceTrustSnapshot } from '@/api/daemon/device-trust'

export interface DeviceTrustContextValue {
  deviceGroups: DeviceGroupChoices | null
  snapshot: DeviceTrustSnapshot | null
  loading: boolean
  decisionBusy: boolean
  decisionError: string | null
  localRemovalConfirmationIssueId: string | null
  refresh: () => Promise<void>
  choose: (issueId: string, choiceId: string, confirmLocalRemoval: boolean) => Promise<void>
}

export const DeviceTrustContext = createContext<DeviceTrustContextValue | null>(null)
