import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import i18n from '@/i18n'
import { DeviceTrustModal } from '../DeviceTrustModal'

const snapshot = {
  revision: 2,
  localDeviceId: 'windows',
  localMembership: 'active',
  currentChange: {
    changeId: 'change-1',
    proposedByDeviceId: 'mac',
    targetDeviceIds: ['phone'],
    includesLocalDevice: false,
    applyImpact: {
      usableDeviceIds: ['mac', 'windows'],
      pausedDeviceIds: [],
      localDeviceOutcome: 'active',
      requiresRejoinDeviceIds: ['phone'],
    },
    keepCurrentImpact: {
      usableDeviceIds: ['windows', 'phone'],
      pausedDeviceIds: ['mac'],
      localDeviceOutcome: 'active',
      requiresRejoinDeviceIds: [],
    },
    allowedChoices: ['apply_change', 'keep_current_device_group'],
    blockedReason: null,
  },
  devices: [
    {
      deviceId: 'mac',
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
      deviceId: 'phone',
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
  ],
  recovery: 'not_available_in_this_version',
  allowedActions: [],
  blockedReason: null,
  updatedAtMs: 1,
} satisfies DeviceTrustSnapshot

describe('DeviceTrustModal', () => {
  it('cannot be dismissed and requires confirmation before keeping the current space', () => {
    const decide = vi.fn()
    render(<DeviceTrustModal snapshot={snapshot} busy={false} error={null} onDecide={decide} />)
    expect(screen.queryByRole('button', { name: /close/i })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: i18n.t('deviceTrust.actions.keep') }))
    expect(decide).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: i18n.t('deviceTrust.actions.confirmKeep') }))
    expect(decide).toHaveBeenCalledWith('keep_current_device_group', false)
  })
})
