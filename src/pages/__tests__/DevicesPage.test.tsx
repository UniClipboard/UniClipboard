import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import DevicesPage from '@/pages/DevicesPage'

const dispatchMock = vi.fn()

vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => dispatchMock,
}))

vi.mock('@/store/slices/devicesSlice', () => ({
  fetchLocalDeviceInfo: vi.fn(() => ({ type: 'devices/fetchLocalDeviceInfo' })),
  fetchSpaceMembers: vi.fn(() => ({ type: 'devices/fetchSpaceMembers' })),
}))

vi.mock('@/components', () => ({
  SpaceMembersPanel: () => <div data-testid="space-members-panel">SpaceMembersPanel</div>,
  ThisDeviceCard: () => <div data-testid="this-device-card">ThisDeviceCard</div>,
  MobileSyncDevicesPanel: () => <div data-testid="mobile-sync-panel">MobileSyncDevicesPanel</div>,
}))

describe('DevicesPage', () => {
  it('renders ThisDeviceCard, SpaceMembersPanel and MobileSyncDevicesPanel', () => {
    render(<DevicesPage />)

    expect(screen.getByTestId('this-device-card')).toBeInTheDocument()
    expect(screen.getByTestId('space-members-panel')).toBeInTheDocument()
    expect(screen.getByTestId('mobile-sync-panel')).toBeInTheDocument()
  })

  it('dispatches fetchLocalDeviceInfo and fetchSpaceMembers on mount', () => {
    render(<DevicesPage />)

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchLocalDeviceInfo' })
    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchSpaceMembers' })
  })

  it('does not render legacy sections', () => {
    render(<DevicesPage />)

    expect(screen.queryByText('Device Management')).not.toBeInTheDocument()
    expect(screen.queryByText('Pairing Requests')).not.toBeInTheDocument()
  })
})
