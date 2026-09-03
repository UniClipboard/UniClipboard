import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SwitchSpaceDialog from '@/components/device/SwitchSpaceDialog'
import i18n from '@/i18n'

const switchSpace = vi.hoisted(() => vi.fn())
const cancelJoinSpace = vi.hoisted(() => vi.fn())

vi.mock('@/api/daemon/setupV2', () => ({
  switchSpace: (...args: unknown[]) => switchSpace(...args),
  cancelJoinSpace: (...args: unknown[]) => cancelJoinSpace(...args),
  SetupV2Error: class SetupV2Error extends Error {},
}))

vi.mock('@/api/daemon/device-trust', () => ({
  getDeviceTrustSnapshot: vi.fn(() => Promise.resolve({ currentJoin: null })),
}))
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn(() => () => undefined),
    onReconnect: vi.fn(() => () => undefined),
  },
}))
vi.mock('@/store/hooks', () => ({ useAppDispatch: () => vi.fn() }))
vi.mock('@/store/slices/devicesSlice', () => ({
  fetchSpaceMembers: vi.fn(() => ({ type: 'devices/fetchSpaceMembers' })),
  fetchLocalDeviceInfo: vi.fn(() => ({ type: 'devices/fetchLocalDeviceInfo' })),
}))

describe('SwitchSpaceDialog durable admission', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    await i18n.changeLanguage('en-US')
    switchSpace.mockResolvedValue({
      status: 'pending',
      joinId: 'join-123',
      targetSpaceId: 'space-123',
      sponsorDeviceId: 'sponsor-123',
      sponsorIdentityFingerprint: 'fingerprint',
      cancelRequested: false,
    })
    cancelJoinSpace.mockResolvedValue({
      status: 'pending',
      joinId: 'join-123',
      targetSpaceId: 'space-123',
      sponsorDeviceId: 'sponsor-123',
      sponsorIdentityFingerprint: 'fingerprint',
      cancelRequested: true,
    })
  })

  it('shows a cancel action while a switch admission is pending', async () => {
    render(
      <I18nextProvider i18n={i18n}>
        <SwitchSpaceDialog open onOpenChange={vi.fn()} />
      </I18nextProvider>
    )

    fireEvent.change(screen.getByLabelText('Invitation code'), {
      target: { value: 'ABCD1234' },
    })
    fireEvent.change(screen.getByLabelText('New space passphrase'), {
      target: { value: 'passphrase' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Switch' }))

    await waitFor(() => {
      expect(screen.getAllByText('Waiting for confirmation')).not.toHaveLength(0)
      expect(screen.getByRole('button', { name: 'Cancel' })).toBeEnabled()
    })
  })

  it('requests cancellation for the pending switch admission', async () => {
    render(
      <I18nextProvider i18n={i18n}>
        <SwitchSpaceDialog open onOpenChange={vi.fn()} />
      </I18nextProvider>
    )

    fireEvent.change(screen.getByLabelText('Invitation code'), {
      target: { value: 'ABCD1234' },
    })
    fireEvent.change(screen.getByLabelText('New space passphrase'), {
      target: { value: 'passphrase' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Switch' }))

    await waitFor(() => screen.getByRole('button', { name: 'Cancel' }))
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))

    await waitFor(() => expect(cancelJoinSpace).toHaveBeenCalledWith('join-123'))
  })
})
