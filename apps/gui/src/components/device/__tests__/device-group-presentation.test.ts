import { describe, expect, it } from 'vitest'
import type { DeviceGroupChoices } from '@/api/daemon/device-trust'
import { presentDeviceGroups } from '@/components/device/device-group-presentation'
import i18n from '@/i18n'

const groups = {
  revision: 1,
  deviceTrust: {
    revision: 1,
    localDeviceId: 'd',
    localMembership: 'active',
    currentChange: null,
    recovery: 'not_available_in_this_version',
    allowedActions: [],
    blockedReason: null,
    updatedAtMs: 0,
    devices: [
      {
        deviceId: 'd',
        displayName: 'd',
        isLocal: true,
        reachability: 'unknown',
        membership: 'active',
        groupRelationship: 'consistent',
        compatibility: 'compatible',
        syncRelationship: 'usable',
        availableActions: [],
        blockedReason: null,
      },
    ],
  },
  issues: [
    {
      issueId: 'issue',
      reason: {
        kind: 'pending_removal',
        detailsComplete: true,
        changes: [
          {
            actor: { deviceId: 'b', displayName: 'b' },
            target: { deviceId: 'c', displayName: 'c' },
            kind: 'removed_device',
            side: 'remote',
          },
        ],
        decisions: [],
      },
      choices: [
        {
          choiceId: 'apply',
          isCurrentGroup: false,
          requiresRePairing: false,
          memberDeviceIds: ['a', 'b', 'd'],
          membersComplete: true,
          members: ['a', 'b', 'd'].map(deviceId => ({
            deviceId,
            displayName: deviceId === 'a' ? 'Remote name' : deviceId,
            active: true,
            isLocal: deviceId === 'd',
          })),
          impact: {
            syncScopeDeviceIds: ['a', 'b'],
            pausedDeviceIds: ['c'],
            pendingConfirmationDeviceIds: ['b'],
            requiresRejoinDeviceIds: ['c'],
            localDeviceOutcome: 'active',
          },
        },
      ],
    },
  ],
} as DeviceGroupChoices

describe('candidate presentation', () => {
  it('extends the suffix when duplicate names also share their last four characters', () => {
    const input = structuredClone(groups)
    input.issues[0].choices[0].members = ['left-0001', 'right-0001'].map(deviceId => ({
      deviceId,
      displayName: 'Same',
      active: true,
      isLocal: false,
    }))
    input.issues[0].choices[0].memberDeviceIds = ['left-0001', 'right-0001']
    const names = presentDeviceGroups(input, i18n.t.bind(i18n)).choices[0].members.split(
      i18n.t('deviceTrust.listSeparator')
    )
    expect(new Set(names).size).toBe(2)
  })
  it('uses candidate names and explicit impact without inventing current sync', () => {
    const view = presentDeviceGroups(groups, i18n.t.bind(i18n))
    expect(view.choices[0].members).toContain('Remote name')
    expect(view.choices[0].members.split(i18n.t('deviceTrust.listSeparator'))).not.toContain('c')
    expect(view.choices[0].paused).toBe('c')
    expect(view.choices[0].pending).toBe('b')
    expect(view.reason).toContain('b')
    expect(view.reason).toContain('c')
  })
  it('treats missing impact and incomplete membership as unknown', () => {
    const input = structuredClone(groups)
    input.issues[0].choices[0] = {
      choiceId: 'old',
      isCurrentGroup: false,
      requiresRePairing: false,
      memberDeviceIds: [],
      membersComplete: false,
    }
    const choice = presentDeviceGroups(input, i18n.t.bind(i18n)).choices[0]
    expect(choice.impactKnown).toBe(false)
    expect(choice.removesLocal).toBe(false)
    expect(choice.members).not.toBe(i18n.t('deviceTrust.modal.noDevices'))
    expect(choice.paused).toBe('')
  })
  it('disambiguates duplicate names across candidate groups', () => {
    const input = structuredClone(groups)
    input.issues[0].choices[0].members![0].displayName = 'b'
    const choice = presentDeviceGroups(input, i18n.t.bind(i18n)).choices[0]
    expect(choice.members).toContain('b · A')
    expect(choice.members).toContain('b · B')
  })
  it('keeps unnamed members identifiable without inventing a name', () => {
    const input = structuredClone(groups)
    input.issues[0].choices[0].members![0].displayName = ''
    expect(presentDeviceGroups(input, i18n.t.bind(i18n)).choices[0].members).toContain(
      `${i18n.t('deviceTrust.presentation.unnamed')} · A`
    )
  })
  it('uses an explicit removed outcome and does not infer it from current membership', () => {
    const input = structuredClone(groups)
    input.issues[0].choices[0].impact!.localDeviceOutcome = 'removed'
    input.issues[0].choices[0].impact!.syncScopeDeviceIds = []
    expect(presentDeviceGroups(input, i18n.t.bind(i18n)).choices[0].removesLocal).toBe(true)
  })
})
