/**
 * Invitation loading must survive StrictMode and each completed dialog session.
 * The inner form resets only after the closing animation has finished.
 */

import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { StrictMode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import AddDeviceDialog from '@/components/device/AddDeviceDialog'
import i18n from '@/i18n'

const getSetupState = vi.fn()
const issuePairingInvitation = vi.fn()
const cancelInvitation = vi.fn()
const getDeviceTrustSnapshot = vi.fn()
const unlockSpaceWithPassphrase = vi.fn()
const { logInfo, logWarn, logError } = vi.hoisted(() => ({
  logInfo: vi.fn(),
  logWarn: vi.fn(),
  logError: vi.fn(),
}))
let deviceTrustHandler: ((event?: { eventType: string }) => void) | undefined
let reconnectHandler: (() => void) | undefined

vi.mock('@/lib/logger', () => ({
  createLogger: () => ({
    info: logInfo,
    warn: logWarn,
    error: logError,
  }),
}))

vi.mock('@/api/daemon/setupV2', () => ({
  getSetupState: () => getSetupState(),
  issuePairingInvitation: () => issuePairingInvitation(),
  cancelInvitation: () => cancelInvitation(),
}))

vi.mock('@/api/daemon/device-trust', () => ({
  getDeviceTrustSnapshot: () => getDeviceTrustSnapshot(),
}))

vi.mock('@/api/security', () => ({
  unlockSpaceWithPassphrase: (passphrase: string) => unlockSpaceWithPassphrase(passphrase),
  isUnlockSpaceError: (error: unknown) =>
    typeof error === 'object' && error !== null && 'code' in error,
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn((_topics, callback) => {
      deviceTrustHandler = event => callback(event ?? { eventType: 'device-trust.changed' })
      return () => undefined
    }),
    onReconnect: vi.fn(callback => {
      reconnectHandler = callback
      return () => undefined
    }),
  },
}))

vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => vi.fn(),
}))

vi.mock('@/store/slices/devicesSlice', () => ({
  fetchSpaceMembers: vi.fn(() => ({ type: 'devices/fetchSpaceMembers' })),
}))

describe('AddDeviceDialog invitation issuing', () => {
  beforeAll(async () => {
    if (!i18n.isInitialized) {
      await new Promise<void>(resolve => {
        const handler = () => {
          i18n.off('initialized', handler)
          resolve()
        }
        i18n.on('initialized', handler)
      })
    }
    await i18n.changeLanguage('en-US')
  })

  beforeEach(() => {
    vi.clearAllMocks()
    deviceTrustHandler = undefined
    reconnectHandler = undefined
    getSetupState.mockResolvedValue({
      hasCompleted: true,
      currentInvitation: null,
      deviceName: 'test',
      rePairingRequired: false,
    })
    getDeviceTrustSnapshot.mockResolvedValue({
      localDeviceId: 'local',
      devices: [{ deviceId: 'local', membership: 'active' }],
    })
    issuePairingInvitation.mockResolvedValue({
      code: '123456789',
      expiresAtMs: Date.now() + 300_000,
    })
    cancelInvitation.mockResolvedValue(undefined)
    unlockSpaceWithPassphrase.mockResolvedValue({ spaceId: 'space' })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('issues and renders an invitation when opened under StrictMode', async () => {
    const { rerender } = render(
      <StrictMode>
        <I18nextProvider i18n={i18n}>
          <AddDeviceDialog open={false} onOpenChange={() => undefined} />
        </I18nextProvider>
      </StrictMode>
    )

    rerender(
      <StrictMode>
        <I18nextProvider i18n={i18n}>
          <AddDeviceDialog open onOpenChange={() => undefined} />
        </I18nextProvider>
      </StrictMode>
    )

    await waitFor(() => {
      expect(screen.getByLabelText('123456789')).toBeInTheDocument()
    })
    expect(issuePairingInvitation).toHaveBeenCalledTimes(1)
  })

  it('does not issue an invitation without a device-trust baseline', async () => {
    getDeviceTrustSnapshot.mockRejectedValue(new Error('device trust unavailable'))

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
      </I18nextProvider>
    )

    await waitFor(() => {
      expect(screen.getByText(i18n.t('devices.addDevice.errors.issueFailed'))).toBeInTheDocument()
    })
    expect(issuePairingInvitation).not.toHaveBeenCalled()
  })

  it('confirms the original passphrase before issuing a re-pairing invitation', async () => {
    getSetupState.mockResolvedValue({
      hasCompleted: true,
      currentInvitation: null,
      deviceName: 'test',
      rePairingRequired: true,
    })

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
      </I18nextProvider>
    )

    const input = await screen.findByLabelText(
      i18n.t('devices.addDevice.rePairing.passphraseLabel')
    )
    expect(issuePairingInvitation).not.toHaveBeenCalled()
    fireEvent.change(input, { target: { value: 'original-passphrase' } })
    fireEvent.click(
      screen.getByRole('button', {
        name: i18n.t('devices.addDevice.rePairing.submit'),
      })
    )

    await waitFor(() => expect(screen.getByLabelText('123456789')).toBeInTheDocument())
    expect(unlockSpaceWithPassphrase).toHaveBeenCalledWith('original-passphrase')
    expect(issuePairingInvitation).toHaveBeenCalledOnce()
    expect(logInfo).toHaveBeenCalledWith(
      { event: 'invitation_ready', mode: 'legacy_re_pairing' },
      're-pairing invitation ready'
    )
    expect(JSON.stringify(logInfo.mock.calls)).not.toContain('original-passphrase')
  })

  it('keeps the confirmation step open after a wrong passphrase', async () => {
    getSetupState.mockResolvedValue({
      hasCompleted: true,
      currentInvitation: null,
      deviceName: 'test',
      rePairingRequired: true,
    })
    unlockSpaceWithPassphrase.mockRejectedValue({ code: 'WRONG_PASSPHRASE' })

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
      </I18nextProvider>
    )

    const input = await screen.findByLabelText(
      i18n.t('devices.addDevice.rePairing.passphraseLabel')
    )
    fireEvent.change(input, { target: { value: 'wrong-passphrase' } })
    fireEvent.click(
      screen.getByRole('button', { name: i18n.t('devices.addDevice.rePairing.submit') })
    )

    expect(
      await screen.findByText(i18n.t('devices.addDevice.rePairing.wrongPassphrase'))
    ).toBeInTheDocument()
    expect(input).toHaveValue('wrong-passphrase')
    expect(issuePairingInvitation).not.toHaveBeenCalled()
    expect(logInfo).toHaveBeenCalledWith(
      { error_kind: 'wrong_passphrase', event: 'credentials_rejected' },
      're-pairing credentials rejected'
    )
    expect(JSON.stringify([...logInfo.mock.calls, ...logWarn.mock.calls])).not.toContain(
      'wrong-passphrase'
    )
  })

  it('replaces the invitation with success after a new member is confirmed', async () => {
    const onSuccess = vi.fn()
    getDeviceTrustSnapshot
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [
          { deviceId: 'local', membership: 'active' },
          { deviceId: 'old', membership: 'active' },
        ],
      })
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [
          { deviceId: 'local', membership: 'active' },
          { deviceId: 'new', membership: 'active' },
        ],
      })
    getSetupState
      .mockResolvedValueOnce({
        hasCompleted: true,
        currentInvitation: null,
        deviceName: 'test',
      })
      .mockResolvedValueOnce({
        hasCompleted: true,
        currentInvitation: null,
        deviceName: 'test',
      })
    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} onSuccess={onSuccess} />
      </I18nextProvider>
    )

    await waitFor(() => {
      expect(screen.getByLabelText('123456789')).toBeInTheDocument()
      expect(deviceTrustHandler).toBeTypeOf('function')
    })

    act(() => {
      deviceTrustHandler?.()
    })

    await waitFor(() => {
      expect(screen.queryByLabelText('123456789')).not.toBeInTheDocument()
      expect(screen.getAllByText(i18n.t('devices.addDevice.success.title'))).not.toHaveLength(0)
      expect(onSuccess).toHaveBeenCalledOnce()
    })
    expect(logInfo).toHaveBeenCalledWith(
      { event: 'pairing_confirmed', trigger: 'device_trust_changed' },
      'new device pairing confirmed'
    )
  })

  it('keeps the success state visible briefly, then closes automatically', async () => {
    const onOpenChange = vi.fn()
    getDeviceTrustSnapshot
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [{ deviceId: 'local', membership: 'active' }],
      })
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [
          { deviceId: 'local', membership: 'active' },
          { deviceId: 'peer', membership: 'active' },
        ],
      })

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={onOpenChange} />
      </I18nextProvider>
    )

    await waitFor(() => {
      expect(screen.getByLabelText('123456789')).toBeInTheDocument()
      expect(deviceTrustHandler).toBeTypeOf('function')
    })

    vi.useFakeTimers()
    act(() => {
      deviceTrustHandler?.()
    })

    await act(async () => {
      await Promise.resolve()
    })
    expect(screen.getAllByText(i18n.t('devices.addDevice.success.title'))).not.toHaveLength(0)
    expect(onOpenChange).not.toHaveBeenCalled()

    act(() => {
      vi.advanceTimersByTime(4999)
    })
    expect(onOpenChange).not.toHaveBeenCalled()

    act(() => {
      vi.advanceTimersByTime(1)
    })
    expect(onOpenChange).toHaveBeenCalledOnce()
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it('shows success when a new member is active even if the issued invitation remains', async () => {
    getDeviceTrustSnapshot
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [{ deviceId: 'local', membership: 'active' }],
      })
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [
          { deviceId: 'local', membership: 'active' },
          { deviceId: 'peer', membership: 'active' },
        ],
      })
    getSetupState
      .mockResolvedValueOnce({
        hasCompleted: true,
        currentInvitation: null,
        deviceName: 'test',
      })
      .mockResolvedValueOnce({
        hasCompleted: true,
        currentInvitation: {
          code: '123456789',
          expiresAtMs: Date.now() + 300_000,
        },
        deviceName: 'test',
      })

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
      </I18nextProvider>
    )

    await waitFor(() => expect(deviceTrustHandler).toBeTypeOf('function'))
    act(() => deviceTrustHandler?.())

    await waitFor(() => {
      expect(screen.getAllByText(i18n.t('devices.addDevice.success.title'))).not.toHaveLength(0)
    })
    expect(cancelInvitation).toHaveBeenCalledOnce()
  })

  it('rechecks the completed invitation after reconnecting', async () => {
    getDeviceTrustSnapshot
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [{ deviceId: 'local', membership: 'active' }],
      })
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [
          { deviceId: 'local', membership: 'active' },
          { deviceId: 'peer', membership: 'active' },
        ],
      })

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
      </I18nextProvider>
    )

    await waitFor(() => expect(reconnectHandler).toBeTypeOf('function'))
    act(() => reconnectHandler?.())

    await waitFor(() => {
      expect(screen.getAllByText(i18n.t('devices.addDevice.success.title'))).not.toHaveLength(0)
    })
  })

  it('rechecks the completed invitation after a global refresh notification', async () => {
    getDeviceTrustSnapshot
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [{ deviceId: 'local', membership: 'active' }],
      })
      .mockResolvedValueOnce({
        localDeviceId: 'local',
        devices: [
          { deviceId: 'local', membership: 'active' },
          { deviceId: 'peer', membership: 'active' },
        ],
      })

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
      </I18nextProvider>
    )

    await waitFor(() => expect(deviceTrustHandler).toBeTypeOf('function'))
    act(() => deviceTrustHandler?.({ eventType: 'system.refresh_required' }))

    await waitFor(() => {
      expect(screen.getAllByText(i18n.t('devices.addDevice.success.title'))).not.toHaveLength(0)
    })
  })
})
