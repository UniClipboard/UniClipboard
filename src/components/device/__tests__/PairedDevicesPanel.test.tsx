import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import PairedDevicesPanel from '../PairedDevicesPanel'
import * as p2pApi from '@/api/p2p'
import i18n from '@/i18n'

vi.mock('../DeviceSettingsPanel', () => ({
  default: ({ deviceName }: { deviceName: string }) => (
    <div data-testid="device-settings-panel">Settings for {deviceName}</div>
  ),
}))

const dispatchMock = vi.fn()
const useAppSelectorMock = vi.fn()

vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => dispatchMock,
  useAppSelector: (selector: (state: { devices: unknown }) => unknown) =>
    useAppSelectorMock(selector),
}))

vi.mock('@/api/p2p', () => ({
  onP2PPeerConnectionChanged: vi.fn(() => Promise.resolve(() => {})),
  onP2PPeerNameUpdated: vi.fn(() => Promise.resolve(() => {})),
  unpairP2PDevice: vi.fn(),
}))

vi.mock('@/store/slices/devicesSlice', () => ({
  fetchPairedDevices: vi.fn(() => ({ type: 'devices/fetchPairedDevices' })),
  clearPairedDevicesError: vi.fn(() => ({ type: 'devices/clearPairedDevicesError' })),
  updatePeerConnectionStatus: vi.fn(),
  updatePeerDeviceName: vi.fn(),
}))

describe('PairedDevicesPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAppSelectorMock.mockImplementation(() => {
      return {
        pairedDevices: [],
        pairedDevicesLoading: false,
        pairedDevicesError: null,
      }
    })
  })

  it('renders empty state with discovery message when no devices are paired', () => {
    render(<PairedDevicesPanel />)

    expect(screen.getByText(i18n.t('devices.list.empty.title'))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('devices.list.empty.description'))).toBeInTheDocument()
  })

  it('renders paired devices in a list', () => {
    const pairedDevices = [
      { peerId: 'device-1', deviceName: 'iPhone 13', connected: true },
      { peerId: 'device-2', deviceName: 'MacBook Pro', connected: false },
    ]

    useAppSelectorMock.mockImplementation(() => {
      return {
        pairedDevices,
        pairedDevicesLoading: false,
        pairedDevicesError: null,
      }
    })

    render(<PairedDevicesPanel />)

    expect(screen.getByText('iPhone 13')).toBeInTheDocument()
    expect(screen.getByText('MacBook Pro')).toBeInTheDocument()
    expect(screen.getByText(i18n.t('devices.list.status.online'))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('devices.list.status.offline'))).toBeInTheDocument()
  })

  it('expands device row on click (accordion behavior)', async () => {
    const pairedDevices = [
      { peerId: 'device-1', deviceName: 'iPhone 13', connected: true },
      { peerId: 'device-2', deviceName: 'MacBook Pro', connected: false },
    ]

    useAppSelectorMock.mockImplementation(() => {
      return {
        pairedDevices,
        pairedDevicesLoading: false,
        pairedDevicesError: null,
      }
    })

    render(<PairedDevicesPanel />)

    expect(screen.queryByTestId('device-settings-panel')).not.toBeInTheDocument()

    fireEvent.click(screen.getByText('iPhone 13'))

    expect(await screen.findByText('Settings for iPhone 13')).toBeInTheDocument()

    fireEvent.click(screen.getByText('MacBook Pro'))

    expect(await screen.findByText('Settings for MacBook Pro')).toBeInTheDocument()
    await waitFor(() => {
      expect(screen.queryByText('Settings for iPhone 13')).not.toBeInTheDocument()
    })
  })

  it('unpair button click does not toggle expansion', async () => {
    const pairedDevices = [{ peerId: 'device-1', deviceName: 'iPhone 13', connected: true }]

    useAppSelectorMock.mockImplementation(() => {
      return {
        pairedDevices,
        pairedDevicesLoading: false,
        pairedDevicesError: null,
      }
    })

    render(<PairedDevicesPanel />)

    const unpairButton = screen.getByTitle(i18n.t('devices.list.actions.unpair'))

    fireEvent.click(unpairButton)

    expect(p2pApi.unpairP2PDevice).toHaveBeenCalledWith('device-1')

    expect(screen.queryByTestId('device-settings-panel')).not.toBeInTheDocument()
  })
})
