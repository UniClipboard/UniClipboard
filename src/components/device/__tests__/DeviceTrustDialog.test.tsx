import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import { DeviceTrustDialog } from '@/components/device/DeviceTrustDialog'
import i18n from '@/i18n'

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

describe('DeviceTrustDialog', () => {
  it('selects an option from the cards and confirms it with the only footer button', () => {
    const decide = vi.fn()
    render(<DeviceTrustDialog snapshot={snapshot} busy={false} error={null} onDecide={decide} />)
    expect(screen.queryByRole('button', { name: /close/i })).not.toBeInTheDocument()
    const options = screen.getAllByRole('radio')
    expect(screen.getByText(i18n.t('deviceTrust.modal.title'))).toBeInTheDocument()
    expect(screen.getAllByText(i18n.t('deviceTrust.modal.continueSyncing'))).toHaveLength(2)
    expect(screen.getAllByText(i18n.t('deviceTrust.modal.stopSyncing'))).toHaveLength(2)
    expect(
      screen.getAllByText(i18n.t('deviceTrust.modal.continueSyncing'))[0].parentElement
    ).toHaveClass('text-emerald-600')
    expect(
      screen.getAllByText(i18n.t('deviceTrust.modal.stopSyncing'))[0].parentElement
    ).toHaveClass('text-destructive')
    expect(screen.queryByText(/Windows/)).not.toBeInTheDocument()
    expect(options[0]).toHaveAttribute('aria-checked', 'true')
    fireEvent.click(options[1])
    expect(options[1]).toHaveAttribute('aria-checked', 'true')
    const confirm = screen.getByRole('button', { name: i18n.t('deviceTrust.actions.confirm') })
    expect(screen.getAllByRole('button')).toHaveLength(1)
    fireEvent.click(confirm)
    expect(decide).toHaveBeenCalledWith('keep_current_device_group', false)
  })

  it('confirms local removal when applying a change that includes this device', () => {
    const decide = vi.fn()
    const localRemovalSnapshot: DeviceTrustSnapshot = {
      ...snapshot,
      currentChange: {
        ...snapshot.currentChange,
        includesLocalDevice: true,
        applyImpact: { ...snapshot.currentChange.applyImpact, localDeviceOutcome: 'removed' },
      },
    }
    render(
      <DeviceTrustDialog
        snapshot={localRemovalSnapshot}
        busy={false}
        error={null}
        onDecide={decide}
      />
    )
    expect(
      screen.getByRole('radio', { name: new RegExp(i18n.t('deviceTrust.modal.leaveTitle')) })
    ).toBeInTheDocument()
    expect(
      screen.getByRole('radio', { name: new RegExp(i18n.t('deviceTrust.modal.stayTitle')) })
    ).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: i18n.t('deviceTrust.actions.confirm') }))
    expect(decide).toHaveBeenCalledWith('apply_change', true)
  })

  it('resets the selected choice when the pending change changes', () => {
    const decide = vi.fn()
    const { rerender } = render(
      <DeviceTrustDialog snapshot={snapshot} busy={false} error={null} onDecide={decide} />
    )
    fireEvent.click(screen.getAllByRole('radio')[1])

    rerender(
      <DeviceTrustDialog
        snapshot={{
          ...snapshot,
          currentChange: { ...snapshot.currentChange, changeId: 'change-2' },
        }}
        busy={false}
        error={null}
        onDecide={decide}
      />
    )

    expect(screen.getAllByRole('radio')[0]).toHaveAttribute('aria-checked', 'true')
  })

  it('only shows choices allowed by the current change', () => {
    const decide = vi.fn()
    render(
      <DeviceTrustDialog
        snapshot={{
          ...snapshot,
          currentChange: {
            ...snapshot.currentChange,
            allowedChoices: ['keep_current_device_group'],
          },
        }}
        busy={false}
        error={null}
        onDecide={decide}
      />
    )

    expect(screen.getAllByRole('radio')).toHaveLength(1)
    expect(screen.getByRole('radio')).toHaveAccessibleName(
      new RegExp(i18n.t('deviceTrust.modal.keepTitle'))
    )
  })
})
