import { createContext } from 'react'
import type { DeviceGroupChoicesFailure } from '@/api/daemon/device-group-choices-failure'
import type {
  DeviceGroupChoices,
  DeviceGroupChoiceOutcome,
  DeviceTrustSnapshot,
} from '@/api/daemon/device-trust'

export interface DeviceGroupDecision {
  groups: DeviceGroupChoices
  issueId: string
  choiceId: string
  outcome: DeviceGroupChoiceOutcome | 'submitting' | 'uncertain'
}

export interface DeviceTrustContextValue {
  deviceGroups: DeviceGroupChoices | null
  snapshot: DeviceTrustSnapshot | null
  loading: boolean
  refreshError?: string | null
  refreshFailure?: DeviceGroupChoicesFailure | null
  decisionBusy: boolean
  decisionError: string | null
  localRemovalConfirmationIssueId: string | null
  localRemovalConfirmationChoiceId: string | null
  decision: DeviceGroupDecision | null
  acknowledgeDecision: () => Promise<void>
  cancelLocalConfirmation: () => void
  refresh: () => Promise<void>
  choose: (issueId: string, choiceId: string, confirmLocalRemoval: boolean) => Promise<void>
}

export const DeviceTrustContext = createContext<DeviceTrustContextValue | null>(null)
