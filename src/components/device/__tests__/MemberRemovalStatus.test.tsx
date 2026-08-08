import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { MemberRemoval } from '@/api/daemon/member'
import MemberRemovalStatus from '@/components/device/MemberRemovalStatus'
import i18n from '@/i18n'

const pendingRemoval: MemberRemoval = {
  revocationId: 'removal-1',
  outcome: 'applied',
  pendingRecipients: 1,
  removedDeviceIds: ['removed-device'],
  pendingRecipientDeviceIds: ['retained-device'],
  updatedAtMs: 1_700_000_000_000,
}

describe('MemberRemovalStatus', () => {
  it('keeps a removal visible until every retained device confirms it', () => {
    render(
      <MemberRemovalStatus
        removal={pendingRemoval}
        deviceNames={new Map([['retained-device', 'Office Mac']])}
        onPermanentLoss={vi.fn()}
      />
    )

    expect(screen.getByText(i18n.t('devices.memberRemoval.pending.title'))).toBeInTheDocument()
    expect(screen.getByText('Office Mac')).toBeInTheDocument()
    expect(
      screen.queryByText(i18n.t('devices.memberRemoval.complete.title'))
    ).not.toBeInTheDocument()
  })

  it('only offers permanent-loss recovery for a currently pending device', async () => {
    const onPermanentLoss = vi.fn()
    const user = userEvent.setup()
    render(
      <MemberRemovalStatus
        removal={pendingRemoval}
        deviceNames={new Map([['retained-device', 'Office Mac']])}
        onPermanentLoss={onPermanentLoss}
      />
    )

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.action') })
    )

    expect(onPermanentLoss).toHaveBeenCalledWith(pendingRemoval, ['retained-device'])
  })

  it('clearly identifies the state that requires permanent-loss recovery', () => {
    render(
      <MemberRemovalStatus
        removal={{ ...pendingRemoval, outcome: 'recovery_required' }}
        deviceNames={new Map([['retained-device', 'Office Mac']])}
        onPermanentLoss={vi.fn()}
      />
    )

    expect(screen.getByText(i18n.t('devices.memberRemoval.recovery.title'))).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.memberRemoval.recovery.description'))
    ).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.action') })
    ).not.toBeInTheDocument()
  })

  it('shows recovery progress without allowing permanent loss', () => {
    render(
      <MemberRemovalStatus
        removal={{ ...pendingRemoval, outcome: 'recovering', pendingRecipients: 0 }}
        deviceNames={new Map()}
        onPermanentLoss={vi.fn()}
      />
    )

    expect(screen.getByText(i18n.t('devices.memberRemoval.recovering.title'))).toBeInTheDocument()
    expect(
      screen.getByText(i18n.t('devices.memberRemoval.recovering.description'))
    ).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.action') })
    ).not.toBeInTheDocument()
  })

  it('does not offer permanent-loss recovery after completion', () => {
    render(
      <MemberRemovalStatus
        removal={{
          ...pendingRemoval,
          outcome: 'complete',
          pendingRecipients: 0,
          pendingRecipientDeviceIds: [],
        }}
        deviceNames={new Map()}
        onPermanentLoss={vi.fn()}
      />
    )

    expect(screen.getByText(i18n.t('devices.memberRemoval.complete.title'))).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.action') })
    ).not.toBeInTheDocument()
  })
})
