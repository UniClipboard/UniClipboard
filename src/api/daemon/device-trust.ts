import { daemonClient } from '@/api/daemon/client'
import type { JoinSpaceResponse } from '@/api/daemon/setupV2'
import {
  chooseDeviceGroup as chooseDeviceGroupSdk,
  getDeviceGroupChoices as getDeviceGroupChoicesSdk,
} from '@/api/generated/sdk.gen'
import type {
  DeviceCompatibilityDto,
  DeviceGroupChoiceIssueDto,
  DeviceGroupChoiceOptionDto,
  DeviceGroupChoiceOutcomeDto,
  DeviceGroupChoiceResultDto,
  DeviceGroupChoicesDto,
  DeviceGroupRelationshipDto,
  DeviceMembershipDto,
  DeviceReachabilityDto,
  DeviceSyncRelationshipDto,
  DeviceTrustActionDto,
  DeviceTrustChangeDto,
  DeviceTrustChoiceDto,
  DeviceTrustImpactDto,
  DeviceTrustRelationshipDto,
  DeviceTrustSnapshotDto,
  DeviceTrustUnavailableReasonDto,
  JoinSpaceResponse as GeneratedJoinSpaceResponse,
  PendingInboundMemberDto,
} from '@/api/generated/types.gen'

export type DeviceMembership = DeviceMembershipDto
export type DeviceReachability = DeviceReachabilityDto
export type DeviceGroupRelationship = DeviceGroupRelationshipDto
export type DeviceCompatibility = DeviceCompatibilityDto
export type DeviceSyncRelationship = DeviceSyncRelationshipDto
export type DeviceTrustChoice = DeviceTrustChoiceDto
export type DeviceTrustAction = DeviceTrustActionDto
export type DeviceTrustUnavailableReason = DeviceTrustUnavailableReasonDto
export type DeviceTrustImpact = DeviceTrustImpactDto
export type DeviceTrustChange = DeviceTrustChangeDto
export type DeviceTrustRelationship = DeviceTrustRelationshipDto
export type PendingInboundMember = PendingInboundMemberDto
export type DeviceTrustSnapshot = Omit<DeviceTrustSnapshotDto, 'currentJoin'> & {
  currentJoin?: JoinSpaceResponse | null
}
export type DeviceGroupChoice = DeviceGroupChoiceOptionDto
export type DeviceGroupIssue = DeviceGroupChoiceIssueDto
export type DeviceGroupChoices = Omit<DeviceGroupChoicesDto, 'deviceTrust'> & {
  deviceTrust: DeviceTrustSnapshot
}
export type DeviceGroupChoiceOutcome = DeviceGroupChoiceOutcomeDto
export type DeviceGroupChoiceResult = DeviceGroupChoiceResultDto

function normalizeJoinSpaceResponse(
  response: GeneratedJoinSpaceResponse | null | undefined
): JoinSpaceResponse | null | undefined {
  if (!response || response.status === 'rejected') return response
  if (response.status === 'pending') {
    return {
      ...response,
      targetSpaceId: response.targetSpaceId ?? null,
      sponsorDeviceId: response.sponsorDeviceId ?? null,
      sponsorIdentityFingerprint: response.sponsorIdentityFingerprint ?? null,
    }
  }
  return {
    ...response,
    joinedSpace: {
      ...response.joinedSpace,
      migratedRecords: response.joinedSpace.migratedRecords ?? null,
      preservedUnreadableRecords: response.joinedSpace.preservedUnreadableRecords ?? null,
    },
  }
}

export async function getDeviceGroupChoices(): Promise<DeviceGroupChoices> {
  const result = (await daemonClient.callEnveloped(() =>
    getDeviceGroupChoicesSdk({ throwOnError: true })
  )) as DeviceGroupChoicesDto
  return {
    ...result,
    deviceTrust: {
      ...result.deviceTrust,
      currentJoin: normalizeJoinSpaceResponse(result.deviceTrust.currentJoin),
    },
  }
}

export async function getDeviceTrustSnapshot(): Promise<DeviceTrustSnapshot> {
  return (await getDeviceGroupChoices()).deviceTrust
}

export async function chooseDeviceGroup(
  issueId: string,
  choiceId: string,
  expectedRevision: number,
  confirmLocalRemoval: boolean
): Promise<DeviceGroupChoiceResult> {
  return daemonClient.callEnveloped(() =>
    chooseDeviceGroupSdk({
      body: { issueId, choiceId, expectedRevision, confirmLocalRemoval },
      throwOnError: true,
    })
  ) as Promise<DeviceGroupChoiceResult>
}
