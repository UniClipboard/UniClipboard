import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
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
  it('selects with Space and submits with Enter', async () => {
    const user = userEvent.setup()
    const choose = vi.fn()
    render(
      <DeviceTrustDialog deviceGroups={deviceGroups} busy={false} error={null} onChoose={choose} />
    )
    screen.getAllByRole('radio')[0].focus()
    await user.keyboard(' ')
    expect(screen.getAllByRole('radio')[0]).toHaveAttribute('aria-checked', 'true')
    screen.getByTestId('device-trust-confirm').focus()
    await user.keyboard('{Enter}')
    expect(choose).toHaveBeenCalledWith('p:change-1', 'apply', false)
  })
  it('keeps the selection when only confirmation progress changes', () => {
    const groups: DeviceGroupChoices = structuredClone(deviceGroups)
    groups.issues[0].choices[0].impact = {
      localDeviceOutcome: 'active',
      syncScopeDeviceIds: ['mac'],
      pausedDeviceIds: ['phone'],
      requiresRejoinDeviceIds: [],
      pendingConfirmationDeviceIds: ['mac'],
    }
    const { rerender } = render(
      <DeviceTrustDialog deviceGroups={groups} busy={false} error={null} onChoose={vi.fn()} />
    )
    fireEvent.click(screen.getAllByRole('radio')[0])
    const next = structuredClone(groups)
    next.issues[0].choices[0].impact!.pendingConfirmationDeviceIds = []
    rerender(<DeviceTrustDialog deviceGroups={next} busy={false} error={null} onChoose={vi.fn()} />)
    expect(screen.getAllByRole('radio')[0]).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByTestId('device-trust-confirm')).toBeEnabled()
  })
  it('keeps local removal concise and reveals details only on request', () => {
    const groups: DeviceGroupChoices = structuredClone(deviceGroups)
    groups.issues[0].reason = {
      kind: 'pending_removal',
      detailsComplete: true,
      decisions: [],
      changes: [
        {
          kind: 'removed_device',
          side: 'remote',
          actor: { deviceId: 'mac', displayName: 'Mac' },
          target: { deviceId: 'windows', displayName: 'Windows' },
        },
      ],
    }
    groups.issues[0].choices[0].impact = {
      localDeviceOutcome: 'removed',
      syncScopeDeviceIds: [],
      pausedDeviceIds: ['mac', 'phone'],
      pendingConfirmationDeviceIds: [],
      requiresRejoinDeviceIds: ['windows'],
    }
    groups.issues[0].choices[1].impact = {
      localDeviceOutcome: 'active',
      syncScopeDeviceIds: ['phone'],
      pausedDeviceIds: ['mac'],
      pendingConfirmationDeviceIds: [],
      requiresRejoinDeviceIds: [],
    }
    const choose = vi.fn()
    render(<DeviceTrustDialog deviceGroups={groups} busy={false} error={null} onChoose={choose} />)
    expect(screen.getByRole('heading')).toHaveTextContent(
      i18n.t('deviceTrust.presentation.localRemovalTitle', { actor: 'Mac' })
    )
    expect(screen.getAllByRole('radio')[0]).toHaveTextContent(
      i18n.t('deviceTrust.presentation.leave')
    )
    expect(
      screen.queryByText(i18n.t('deviceTrust.modal.confirmLocalRemoval'))
    ).not.toBeInTheDocument()
    expect(screen.getAllByTestId('choice-members')[0]).not.toBeVisible()
    fireEvent.click(screen.getByText(i18n.t('deviceTrust.presentation.showDevices')))
    expect(screen.getAllByTestId('choice-members')[0]).toBeVisible()
    fireEvent.click(screen.getAllByRole('radio')[0])
    fireEvent.click(screen.getByTestId('device-trust-confirm'))
    expect(choose).not.toHaveBeenCalled()
    expect(screen.getByTestId('device-trust-local-removal-warning')).toBeVisible()
    fireEvent.click(screen.getByTestId('device-trust-confirm'))
    expect(choose).toHaveBeenCalledWith('p:change-1', 'apply', true)
  })
  it('requires an explicit initial selection', () => {
    render(
      <DeviceTrustDialog deviceGroups={deviceGroups} busy={false} error={null} onChoose={vi.fn()} />
    )
    expect(screen.getByTestId('device-trust-confirm')).toBeDisabled()
    for (const option of screen.getAllByRole('radio'))
      expect(option).toHaveAttribute('aria-checked', 'false')
  })
  it('submits the returned issue and choice ids', () => {
    const choose = vi.fn()
    render(
      <DeviceTrustDialog deviceGroups={deviceGroups} busy={false} error={null} onChoose={choose} />
    )
    const options = screen.getAllByRole('radio')
    fireEvent.click(options[1])
    fireEvent.click(screen.getByTestId('device-trust-confirm'))

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
    fireEvent.click(screen.getAllByRole('radio')[0])
    fireEvent.click(screen.getByTestId('device-trust-confirm'))
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

    expect(screen.getByRole('radiogroup')).toHaveAttribute('tabindex', '-1')
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

    expect(screen.getAllByRole('radio')[0]).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByTestId('device-trust-confirm')).toBeDisabled()
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

    expect(
      screen.queryByText(i18n.t('deviceTrust.modal.confirmLocalRemoval'))
    ).not.toBeInTheDocument()
    fireEvent.click(screen.getByText(i18n.t('deviceTrust.presentation.showDevices')))
    expect(screen.getAllByText(/Mac/).length).toBeGreaterThan(0)
    expect(screen.getAllByText('Phone').length).toBeGreaterThan(0)
    fireEvent.click(screen.getAllByRole('radio')[1])
    fireEvent.click(screen.getByTestId('device-trust-confirm'))
    expect(screen.getByTestId('device-trust-local-removal-warning')).toBeVisible()
  })
})
