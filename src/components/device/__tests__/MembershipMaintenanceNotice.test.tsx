import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { MembershipMaintenanceNotice } from '@/components/device/MembershipMaintenanceNotice'

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))

describe('MembershipMaintenanceNotice', () => {
  it('shows no warning for a healthy group', () => {
    render(<MembershipMaintenanceNotice health={{ phase: 'healthy' }} onReview={vi.fn()} />)
    expect(screen.queryByTestId('membership-maintenance-alert')).not.toBeInTheDocument()
  })

  it('reports automatic retry without an invented manual retry action', () => {
    render(
      <MembershipMaintenanceNotice
        health={{ phase: 'retrying', nextRetryAtMs: 12345 }}
        onReview={vi.fn()}
      />
    )
    expect(screen.getByText('devices.membershipMaintenance.retrying')).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('offers the Engine recovery action when attention is needed', () => {
    const onReview = vi.fn()
    render(
      <MembershipMaintenanceNotice
        health={{
          phase: 'needs_attention',
          reason: 'membership_history_rejected',
          recovery: 'resolve_device_trust',
        }}
        onReview={onReview}
      />
    )
    expect(screen.getByText('devices.membershipMaintenance.needsAttention')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'devices.membershipMaintenance.review' }))
    expect(onReview).toHaveBeenCalledOnce()
  })
})
