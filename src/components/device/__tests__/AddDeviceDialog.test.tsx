/**
 * AddDeviceDialog —— 邀请码签发的挂载语义回归测试。
 *
 * 对话框在 DevicesPage 里常驻渲染,open 翻转时才重挂载 Inner。这意味着签发
 * 邀请的 effect 的「首次挂载」发生在 open=true 状态下,会被 StrictMode 双跑。
 * 这里用 StrictMode 渲染,确保双跑后仍然能拿到邀请码而不是卡在 loading。
 */

import { act, render, screen, waitFor } from '@testing-library/react'
import { StrictMode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import AddDeviceDialog from '@/components/device/AddDeviceDialog'
import i18n from '@/i18n'

const getSetupState = vi.fn()
const issuePairingInvitation = vi.fn()
const getPairedPeers = vi.fn()
let pairingCompletedHandler: ((event: { success: boolean }) => void) | undefined

vi.mock('@/api/daemon/setupV2', () => ({
  getSetupState: () => getSetupState(),
  issuePairingInvitation: () => issuePairingInvitation(),
  cancelInvitation: vi.fn(() => Promise.resolve()),
}))

vi.mock('@/api/daemon/members', () => ({
  getPairedPeers: () => getPairedPeers(),
}))

vi.mock('@/api/setupEvents', () => ({
  onSetupInvitationRevoked: vi.fn(() => Promise.resolve(() => undefined)),
  onSetupPairingCompleted: vi.fn(callback => {
    pairingCompletedHandler = callback
    return Promise.resolve(() => undefined)
  }),
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
    pairingCompletedHandler = undefined
    getSetupState.mockResolvedValue({
      hasCompleted: true,
      currentInvitation: null,
      deviceName: 'test',
    })
    getPairedPeers.mockResolvedValue([])
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
    expect(issuePairingInvitation).toHaveBeenCalledTimes(1)
  })

  it('does not issue an invitation without a member-count baseline', async () => {
    getPairedPeers.mockRejectedValue(new Error('member list unavailable'))

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

  it('replaces the invitation with success when pairing completes', async () => {
    getPairedPeers.mockResolvedValueOnce([]).mockResolvedValueOnce([{}])
    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={() => undefined} />
      </I18nextProvider>
    )

    await waitFor(() => {
      expect(screen.getByLabelText('123456789')).toBeInTheDocument()
      expect(pairingCompletedHandler).toBeTypeOf('function')
    })

    act(() => {
      pairingCompletedHandler?.({ success: true })
    })

    await waitFor(() => {
      expect(screen.queryByLabelText('123456789')).not.toBeInTheDocument()
      expect(screen.getAllByText(i18n.t('devices.addDevice.success.title'))).not.toHaveLength(0)
    })
  })

  it('keeps the success state visible briefly, then closes automatically', async () => {
    const onOpenChange = vi.fn()
    getPairedPeers.mockResolvedValueOnce([]).mockResolvedValueOnce([{}])

    render(
      <I18nextProvider i18n={i18n}>
        <AddDeviceDialog open onOpenChange={onOpenChange} />
      </I18nextProvider>
    )

    await waitFor(() => {
      expect(screen.getByLabelText('123456789')).toBeInTheDocument()
      expect(pairingCompletedHandler).toBeTypeOf('function')
    })

    vi.useFakeTimers()
    act(() => {
      pairingCompletedHandler?.({ success: true })
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
})
