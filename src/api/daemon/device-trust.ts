import type { JoinSpaceResponse } from '@/api/daemon/setupV2'

export type DeviceMembership = 'active' | 'removed' | 'unavailable' | 'unknown'
export type DeviceReachability = 'online' | 'offline' | 'unknown'
export type DeviceGroupRelationship =
  | 'consistent'
  | 'pending_local_decision'
  | 'diverged'
  | 'unverifiable'
  | 'unknown'
export type DeviceCompatibility = 'compatible' | 'upgrade_required' | 'unknown'
export type DeviceSyncRelationship =
  | 'usable'
  | 'waiting_for_local_decision'
  | 'paused_group_diverged'
  | 'paused_upgrade_required'
  | 'paused_unverifiable'
  | 'removed_local_device'
  | 'removed_peer_device'
  | 'unknown'
export type DeviceTrustChoice = 'apply_change' | 'keep_current_device_group'
export type DeviceTrustAction =
  | 'apply_current_change'
  | 'keep_current_device_group'
  | 'confirm_apply_removes_local_device'
  | 'rejoin_device_group'
  | 'update_this_device'
export type DeviceTrustUnavailableReason =
  | 'no_current_change'
  | 'change_no_longer_current'
  | 'local_device_confirmation_required'
  | 'local_device_removed'
  | 'recovery_not_available_in_this_version'
  | 'peer_upgrade_required'
  | 'device_facts_unverifiable'
  | 'engine_unavailable'

export interface DeviceTrustImpact {
  usableDeviceIds: string[]
  pausedDeviceIds: string[]
  localDeviceOutcome: DeviceMembership
  requiresRejoinDeviceIds: string[]
}

export interface DeviceTrustChange {
  changeId: string
  proposedByDeviceId: string
  targetDeviceIds: string[]
  includesLocalDevice: boolean
  applyImpact: DeviceTrustImpact
  keepCurrentImpact: DeviceTrustImpact
  allowedChoices: DeviceTrustChoice[]
  blockedReason: DeviceTrustUnavailableReason | null
}

export interface DeviceTrustRelationship {
  deviceId: string
  displayName: string
  isLocal: boolean
  reachability: DeviceReachability
  membership: DeviceMembership
  groupRelationship: DeviceGroupRelationship
  compatibility: DeviceCompatibility
  syncRelationship: DeviceSyncRelationship
  availableActions: DeviceTrustAction[]
  blockedReason: DeviceTrustUnavailableReason | null
}

export interface PendingInboundMember {
  deviceId: string
  displayName: string
}

export interface DeviceTrustSnapshot {
  revision: number
  localDeviceId: string
  localMembership: DeviceMembership
  currentChange: DeviceTrustChange | null
  currentJoin?: JoinSpaceResponse | null
  pendingInboundMember?: PendingInboundMember | null
  devices: DeviceTrustRelationship[]
  recovery: 'not_available_in_this_version'
  allowedActions: DeviceTrustAction[]
  blockedReason: DeviceTrustUnavailableReason | null
  updatedAtMs: number
}

export type DeviceTrustDecision =
  | { kind: 'applied'; changeId: string; snapshot: DeviceTrustSnapshot }
  | {
      kind: 'kept_current_device_group'
      changeId: string
      snapshot: DeviceTrustSnapshot
    }
  | {
      kind: 'already_completed'
      changeId: string
      completedChoice: DeviceTrustChoice
      snapshot: DeviceTrustSnapshot
    }
  | {
      kind: 'state_changed'
      currentChangeId?: string | null
      snapshot: DeviceTrustSnapshot
    }
  | {
      kind: 'local_device_confirmation_required'
      changeId: string
      snapshot: DeviceTrustSnapshot
    }

import {
  decideDeviceTrust as decideDeviceTrustSdk,
  getDeviceTrust as getDeviceTrustSdk,
} from '@/api/generated/sdk.gen'
import { daemonClient } from './client'

export async function getDeviceTrust(): Promise<DeviceTrustSnapshot> {
  return daemonClient.callEnveloped(() =>
    getDeviceTrustSdk({ throwOnError: true })
  ) as Promise<DeviceTrustSnapshot>
}

export async function decideDeviceTrust(
  changeId: string,
  choice: DeviceTrustChoice,
  confirmLocalRemoval: boolean
): Promise<DeviceTrustDecision> {
  return daemonClient.callEnveloped(() =>
    decideDeviceTrustSdk({
      body: { changeId, choice, confirmLocalRemoval },
      throwOnError: true,
    })
  ) as Promise<DeviceTrustDecision>
}
