/**
 * AddDeviceDialog —— 邀请码签发的挂载语义回归测试。
 *
 * 对话框在 DevicesPage 里常驻渲染,open 翻转时才重挂载 Inner。这意味着签发
 * 邀请的 effect 的「首次挂载」发生在 open=true 状态下,会被 StrictMode 双跑。
 * 这里用 StrictMode 渲染,确保双跑后仍然能拿到邀请码而不是卡在 loading。
 */

import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { StrictMode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import AddDeviceDialog from '@/components/device/AddDeviceDialog'
import i18n from '@/i18n'

const getSetupState = vi.fn()
const issuePairingInvitation = vi.fn()
const getDeviceTrust = vi.fn()
let deviceTrustHandler: (() => void) | undefined
let reconnectHandler: (() => void) | undefined

vi.mock('@/api/daemon/setupV2', () => ({
  getSetupState: () => getSetupState(),
  issuePairingInvitation: () => issuePairingInvitation(),
  cancelInvitation: vi.fn(() => Promise.resolve()),
}))

vi.mock('@/api/daemon/device-trust', () => ({
  getDeviceTrust: () => getDeviceTrust(),
}))

vi.mock('qrcode.react', () => ({
  QRCodeSVG: ({ value, 'aria-label': ariaLabel }: { value: string; 'aria-label'?: string }) => (
    <div aria-label={ariaLabel} data-qr-value={value} />
  ),
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn((_topics, callback) => {
      deviceTrustHandler = () => callback({ eventType: 'device-trust.changed' })
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
    })
    getDeviceTrust.mockResolvedValue({
      localDeviceId: 'local',
      devices: [{ deviceId: 'local', membership: 'active' }],
    })
    issuePairingInvitation.mockResolvedValue({
      code: '123456789',
      expiresAtMs: Date.now() + 300_000,
    })
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
    expect(screen.queryByLabelText(i18n.t('devices.addDevice.qrAlt'))).not.toBeInTheDocument()

    fireEvent.change(screen.getByLabelText(i18n.t('devices.addDevice.qrPassphraseLabel')), {
      target: { value: 'correct horse!' },
    })

    expect(screen.getByLabelText(i18n.t('devices.addDevice.qrAlt'))).toHaveAttribute(
      'data-qr-value',
      'uniclipboard://join-space?v=1&code=123456789&pwd=correct%20horse!'
    )
    expect(issuePairingInvitation).toHaveBeenCalledTimes(1)
  })

  it('does not issue an invitation without a device-trust baseline', async () => {
    getDeviceTrust.mockRejectedValue(new Error('device trust unavailable'))

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

  it('replaces the invitation with success after a new member is confirmed', async () => {
    getDeviceTrust
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
      .mockResolvedValueOnce({ hasCompleted: true, currentInvitation: null, deviceName: 'test' })
      .mockResolvedValueOnce({ hasCompleted: true, currentInvitation: null, deviceName: 'test' })
    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
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
    })
  })

  it('keeps the success state visible briefly, then closes automatically', async () => {
    const onOpenChange = vi.fn()
    getDeviceTrust
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
      vi.advanceTimersByTime(1999)
    })
    expect(onOpenChange).not.toHaveBeenCalled()

    act(() => {
      vi.advanceTimersByTime(1)
    })
    expect(onOpenChange).toHaveBeenCalledOnce()
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it('does not show success while the invitation is still active', async () => {
    getDeviceTrust
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
      .mockResolvedValueOnce({ hasCompleted: true, currentInvitation: null, deviceName: 'test' })
      .mockResolvedValueOnce({
        hasCompleted: true,
        currentInvitation: { code: '123456789', expiresAtMs: Date.now() + 300_000 },
        deviceName: 'test',
      })

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
      </I18nextProvider>
    )

    await waitFor(() => expect(deviceTrustHandler).toBeTypeOf('function'))
    act(() => deviceTrustHandler?.())

    await waitFor(() => expect(getDeviceTrust).toHaveBeenCalledTimes(2))
    expect(screen.getByLabelText('123456789')).toBeInTheDocument()
  })

  it('rechecks the completed invitation after reconnecting', async () => {
    getDeviceTrust
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
})
