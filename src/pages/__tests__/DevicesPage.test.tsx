import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import DevicesPage from '@/pages/DevicesPage'

const dispatchMock = vi.fn()

vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => dispatchMock,
}))

vi.mock('@/store/slices/devicesSlice', () => ({
  fetchLocalDeviceInfo: vi.fn(() => ({ type: 'devices/fetchLocalDeviceInfo' })),
}))

vi.mock('@/components', () => ({
  PairedDevicesPanel: () => <div data-testid="paired-devices-panel">PairedDevicesPanel</div>,
  ThisDeviceCard: () => <div data-testid="this-device-card">ThisDeviceCard</div>,
}))

describe('DevicesPage', () => {
  it('renders ThisDeviceCard and PairedDevicesPanel', () => {
    render(<DevicesPage />)

    expect(screen.getByTestId('this-device-card')).toBeInTheDocument()
    expect(screen.getByTestId('paired-devices-panel')).toBeInTheDocument()
  })

  it('dispatches fetchLocalDeviceInfo on mount', () => {
    render(<DevicesPage />)

    expect(dispatchMock).toHaveBeenCalledWith({ type: 'devices/fetchLocalDeviceInfo' })
  })

  it('does not render legacy sections', () => {
    render(<DevicesPage />)

    expect(screen.queryByText('Device Management')).not.toBeInTheDocument()
    expect(screen.queryByText('Pairing Requests')).not.toBeInTheDocument()
  })
})
