import { describe, expect, it } from 'vitest'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { getDeviceLabel, getPendingDecisionView } from '@/components/device/device-trust-model'

const snapshot: DeviceTrustSnapshot = {
  revision: 7,
  localDeviceId: 'device-windows-8F31',
  localMembership: 'active',
  currentChange: {
    changeId: 'change-private',
    proposedByDeviceId: 'device-mac-A7C2',
    targetDeviceIds: ['device-phone-B4D9'],
    includesLocalDevice: false,
    applyImpact: {
      usableDeviceIds: ['device-mac-A7C2', 'device-windows-8F31'],
      pausedDeviceIds: [],
      localDeviceOutcome: 'active',
      requiresRejoinDeviceIds: ['device-phone-B4D9'],
    },
    keepCurrentImpact: {
      usableDeviceIds: ['device-windows-8F31', 'device-phone-B4D9'],
      pausedDeviceIds: ['device-mac-A7C2'],
      localDeviceOutcome: 'active',
      requiresRejoinDeviceIds: [],
    },
    allowedChoices: ['apply_change', 'keep_current_device_group'],
    blockedReason: null,
  },
  devices: [
    {
      deviceId: 'device-mac-A7C2',
      displayName: 'Mac',
      isLocal: false,
      reachability: 'online',
      membership: 'active',
      groupRelationship: 'pending_local_decision',
      compatibility: 'compatible',
      syncRelationship: 'waiting_for_local_decision',
      availableActions: [],
      blockedReason: null,
    },
    {
      deviceId: 'device-phone-B4D9',
      displayName: 'Phone',
      isLocal: false,
      reachability: 'offline',
      membership: 'active',
      groupRelationship: 'consistent',
      compatibility: 'compatible',
      syncRelationship: 'usable',
      availableActions: [],
      blockedReason: null,
    },
    {
      deviceId: 'device-windows-8F31',
      displayName: 'Windows',
      isLocal: true,
      reachability: 'online',
      membership: 'active',
      groupRelationship: 'consistent',
      compatibility: 'compatible',
      syncRelationship: 'usable',
      availableActions: [],
      blockedReason: null,
    },
  ],
  recovery: 'not_available_in_this_version',
  allowedActions: ['apply_current_change', 'keep_current_device_group'],
  blockedReason: null,
  updatedAtMs: 1,
}

describe('device trust product model', () => {
  it('uses Engine impact facts for both user choices', () => {
    const view = getPendingDecisionView(snapshot)

    expect(view?.proposer.label).toBe('Mac')
    expect(view?.targets.map(device => device.label)).toEqual(['Phone'])
    expect(view?.apply.continuesWith.map(device => device.label)).toEqual(['Mac'])
    expect(view?.apply.stopsWith.map(device => device.label)).toEqual(['Phone'])
    expect(view?.keepCurrent.continuesWith.map(device => device.label)).toEqual(['Phone'])
    expect(view?.keepCurrent.stopsWith.map(device => device.label)).toEqual(['Mac'])
  })

  it('adds device suffixes only when display names are duplicated', () => {
    const duplicateNames: DeviceTrustSnapshot = {
      ...snapshot,
      devices: snapshot.devices.map(device =>
        device.deviceId === 'device-phone-B4D9' ? { ...device, displayName: 'Mac' } : device
      ),
    }

    const view = getPendingDecisionView(duplicateNames)

    expect(view?.proposer.label).toBe('Mac · A7C2')
    expect(view?.targets.map(device => device.label)).toEqual(['Mac · B4D9'])
  })

  it('shows no continuing peers when applying a change removes the local device', () => {
    const localRemoval: DeviceTrustSnapshot = {
      ...snapshot,
      currentChange: {
        ...snapshot.currentChange!,
        targetDeviceIds: [snapshot.localDeviceId],
        includesLocalDevice: true,
        applyImpact: {
          ...snapshot.currentChange!.applyImpact,
          usableDeviceIds: ['device-mac-A7C2', 'device-phone-B4D9'],
          localDeviceOutcome: 'removed',
        },
      },
    }

    const view = getPendingDecisionView(localRemoval)

    expect(view?.apply.continuesWith).toEqual([])
    expect(view?.apply.stopsWith.map(device => device.label)).toEqual(['Mac', 'Phone'])
  })

  it('falls back to an unnamed label without persisting another identity', () => {
    expect(getDeviceLabel('', 'device-without-name-C112')).toBe('Unnamed device · C112')
  })
})
