import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { DeviceGroupChoices } from '@/api/daemon/device-trust'
import { DeviceTrustDialog } from '@/components/device/DeviceTrustDialog'
import i18n from '@/i18n'

const deviceGroups = {
  revision: 2,
  deviceTrust: {
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
  },
  issues: [
    {
      issueId: 'p:change-1',
      choices: [
        {
          choiceId: 'apply',
          isCurrentGroup: false,
          requiresRePairing: false,
          memberDeviceIds: ['mac', 'windows'],
          membersComplete: true,
        },
        {
          choiceId: 'keep',
          isCurrentGroup: true,
          requiresRePairing: false,
          memberDeviceIds: ['windows', 'phone'],
          membersComplete: true,
        },
      ],
    },
  ],
} satisfies DeviceGroupChoices

describe('DeviceTrustDialog', () => {
  it('submits the returned issue and choice ids', () => {
    const choose = vi.fn()
    render(
      <DeviceTrustDialog deviceGroups={deviceGroups} busy={false} error={null} onChoose={choose} />
    )
    const options = screen.getAllByRole('radio')
    fireEvent.click(options[1])
    fireEvent.click(screen.getByRole('button', { name: i18n.t('deviceTrust.actions.confirm') }))

    expect(choose).toHaveBeenCalledWith('p:change-1', 'keep', false)
  })

  it('requires two explicit confirmations before removing this device', () => {
    const choose = vi.fn()
    const localRemovalGroups: DeviceGroupChoices = {
      ...deviceGroups,
      deviceTrust: {
        ...deviceGroups.deviceTrust,
        currentChange: {
          ...deviceGroups.deviceTrust.currentChange!,
          includesLocalDevice: true,
          applyImpact: {
            ...deviceGroups.deviceTrust.currentChange!.applyImpact,
            localDeviceOutcome: 'removed',
          },
        },
      },
      issues: [
        {
          ...deviceGroups.issues[0],
          choices: [
            { ...deviceGroups.issues[0].choices[0], memberDeviceIds: ['mac'] },
            deviceGroups.issues[0].choices[1],
          ],
        },
      ],
    }
    const { rerender } = render(
      <DeviceTrustDialog
        deviceGroups={localRemovalGroups}
        busy={false}
        error={null}
        onChoose={choose}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: i18n.t('deviceTrust.actions.confirm') }))
    expect(choose).toHaveBeenCalledWith('p:change-1', 'apply', false)

    rerender(
      <DeviceTrustDialog
        deviceGroups={localRemovalGroups}
        busy={false}
        error={null}
        localRemovalConfirmationIssueId="p:change-1"
        onChoose={choose}
      />
    )
    expect(screen.getByText(i18n.t('deviceTrust.modal.confirmLocalRemoval'))).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: i18n.t('deviceTrust.actions.confirmExit') }))
    expect(choose).toHaveBeenLastCalledWith('p:change-1', 'apply', true)
  })

  it('uses one tab stop and arrow keys to move between choices', () => {
    render(
      <DeviceTrustDialog deviceGroups={deviceGroups} busy={false} error={null} onChoose={vi.fn()} />
    )
    const options = screen.getAllByRole('radio')

    expect(options[0]).toHaveAttribute('tabindex', '0')
    expect(options[1]).toHaveAttribute('tabindex', '-1')
    options[0].focus()
    fireEvent.keyDown(options[0], { key: 'ArrowDown' })

    expect(options[1]).toHaveAttribute('aria-checked', 'true')
    expect(options[1]).toHaveFocus()
  })

  it('resets the selected choice when the current issue changes', () => {
    const { rerender } = render(
      <DeviceTrustDialog deviceGroups={deviceGroups} busy={false} error={null} onChoose={vi.fn()} />
    )
    fireEvent.click(screen.getAllByRole('radio')[1])

    rerender(
      <DeviceTrustDialog
        deviceGroups={{
          ...deviceGroups,
          issues: [{ ...deviceGroups.issues[0], issueId: 'p:change-2' }],
        }}
        busy={false}
        error={null}
        onChoose={vi.fn()}
      />
    )

    expect(screen.getAllByRole('radio')[0]).toHaveAttribute('aria-checked', 'true')
  })

  it('renders arbitrary candidate groups and re-pairing requirements', () => {
    const branchGroups: DeviceGroupChoices = {
      ...deviceGroups,
      deviceTrust: { ...deviceGroups.deviceTrust, currentChange: null },
      issues: [
        {
          issueId: 'c:conflict-1',
          choices: [
            {
              choiceId: 'b:branch-a',
              isCurrentGroup: true,
              requiresRePairing: false,
              memberDeviceIds: ['windows', 'mac'],
              membersComplete: true,
            },
            {
              choiceId: 'b:branch-b',
              isCurrentGroup: false,
              requiresRePairing: true,
              memberDeviceIds: ['phone'],
              membersComplete: true,
            },
          ],
        },
      ],
    }

    render(
      <DeviceTrustDialog deviceGroups={branchGroups} busy={false} error={null} onChoose={vi.fn()} />
    )

    expect(screen.getByText(i18n.t('deviceTrust.modal.requiresRePairing'))).toBeInTheDocument()
    expect(screen.getAllByText('Mac').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Phone').length).toBeGreaterThan(0)
  })
})
