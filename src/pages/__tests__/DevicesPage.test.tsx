import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import DevicesPage from '@/pages/DevicesPage'

vi.mock('@/components', () => ({
  DeviceList: () => (
    <div>
      <div data-testid="device-list">DeviceList</div>
    </div>
  ),
  PairingDialog: () => <div data-testid="pairing-dialog-mock">PairingDialog</div>,
}))

describe('DevicesPage', () => {
  it('renders device list and does not render legacy sections', () => {
    render(<DevicesPage />)

    expect(screen.getByTestId('device-list')).toBeInTheDocument()
    expect(screen.queryByText('Device Management')).not.toBeInTheDocument()
    expect(screen.queryByText('Pairing Requests')).not.toBeInTheDocument()
    expect(screen.queryByText('当前设备')).not.toBeInTheDocument()
  })

  it('does not render pairing dialog entry in page', () => {
    render(<DevicesPage />)

    expect(screen.queryByTestId('pairing-dialog-mock')).not.toBeInTheDocument()
    expect(screen.queryByText('PairingDialog')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Open Pairing' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Trigger Success' })).not.toBeInTheDocument()
  })
})
