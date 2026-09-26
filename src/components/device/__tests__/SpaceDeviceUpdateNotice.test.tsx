import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { SpaceDeviceUpdateNotice } from '@/components/device/SpaceDeviceUpdateNotice'
import { toast } from '@/components/ui/toast'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

describe('SpaceDeviceUpdateNotice', () => {
  it('stays quiet after the status is already completed', () => {
    render(<SpaceDeviceUpdateNotice status={{ phase: 'completed' }} />)
    expect(screen.queryByTestId('space-device-update-status')).not.toBeInTheDocument()
  })

  it('shows one unified message while the space devices are updating', () => {
    render(<SpaceDeviceUpdateNotice status={{ phase: 'updating' }} />)
    expect(screen.getByText('devices.spaceDeviceUpdate.updating.title')).toBeInTheDocument()
    expect(screen.getByText('devices.spaceDeviceUpdate.updating.description')).toBeInTheDocument()
  })

  it('reports automatic retry without inventing a manual retry action', () => {
    render(
      <SpaceDeviceUpdateNotice status={{ phase: 'retryable_failure', nextRetryAtMs: 12345 }} />
    )
    expect(screen.getByText('devices.spaceDeviceUpdate.retrying.title')).toBeInTheDocument()
    expect(screen.getByText('devices.spaceDeviceUpdate.retrying.description')).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('shows a human reason without exposing internal details', () => {
    render(
      <SpaceDeviceUpdateNotice
        status={{
          phase: 'needs_attention',
          reason: 'device_relationship_conflict',
          recovery: 'review_devices',
        }}
      />
    )
    expect(screen.getByText('devices.spaceDeviceUpdate.attention.title')).toBeInTheDocument()
    expect(
      screen.getByText('devices.spaceDeviceUpdate.attention.reasons.device_relationship_conflict')
    ).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('keeps a load failure compact and offers a real retry', () => {
    const onRetry = vi.fn()
    render(<SpaceDeviceUpdateNotice loadFailure="unavailable" onRetry={onRetry} />)
    expect(screen.getByText('devices.spaceDeviceUpdate.unavailable.title')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'devices.list.actions.retry' }))
    expect(onRetry).toHaveBeenCalledOnce()
  })

  it('asks for unlock instead of reporting a generic outage', () => {
    const onRetry = vi.fn()
    render(<SpaceDeviceUpdateNotice loadFailure="unlock_required" onRetry={onRetry} />)
    expect(screen.getByText('devices.spaceDeviceUpdate.unlockRequired.title')).toBeInTheDocument()
    expect(
      screen.getByText('devices.spaceDeviceUpdate.unlockRequired.description')
    ).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'devices.list.actions.retry' })).toBeInTheDocument()
  })

  it('does not offer a retry that cannot resolve a recovery requirement', () => {
    render(<SpaceDeviceUpdateNotice loadFailure="recovery_required" onRetry={vi.fn()} />)
    expect(screen.getByText('devices.spaceDeviceUpdate.recoveryRequired.title')).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('shows the Engine reason for a local identity mismatch without inventing a recovery', () => {
    render(
      <SpaceDeviceUpdateNotice
        status={{
          phase: 'needs_attention',
          reason: 'local_identity_mismatch',
          recovery: null,
        }}
      />
    )
    expect(
      screen.getByText('devices.spaceDeviceUpdate.attention.reasons.local_identity_mismatch')
    ).toBeInTheDocument()
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('announces recovery once when a visible problem becomes completed', () => {
    const success = vi.spyOn(toast, 'success')
    const { rerender } = render(<SpaceDeviceUpdateNotice status={{ phase: 'updating' }} />)
    rerender(<SpaceDeviceUpdateNotice status={{ phase: 'completed' }} />)
    rerender(<SpaceDeviceUpdateNotice status={{ phase: 'completed' }} />)
    expect(success).toHaveBeenCalledOnce()
    expect(success).toHaveBeenCalledWith('devices.spaceDeviceUpdate.completed', {
      id: 'space-device-update-completed',
    })
  })
})
