/**
 * AddDeviceDialog —— 邀请码签发的挂载语义回归测试。
 *
 * 对话框在 DevicesPage 里常驻渲染,open 翻转时才重挂载 Inner。这意味着签发
 * 邀请的 effect 的「首次挂载」发生在 open=true 状态下,会被 StrictMode 双跑。
 * 这里用 StrictMode 渲染,确保双跑后仍然能拿到邀请码而不是卡在 loading。
 */

import { render, screen, waitFor } from '@testing-library/react'
import { StrictMode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import AddDeviceDialog from '@/components/device/AddDeviceDialog'
import i18n from '@/i18n'

const getSetupState = vi.fn()
const issuePairingInvitation = vi.fn()

vi.mock('@/api/daemon/setupV2', () => ({
  getSetupState: () => getSetupState(),
  issuePairingInvitation: () => issuePairingInvitation(),
  cancelInvitation: vi.fn(() => Promise.resolve()),
}))

vi.mock('@/api/setupEvents', () => ({
  onSetupPairingCompleted: vi.fn(() => Promise.resolve(() => undefined)),
  onSetupInvitationRevoked: vi.fn(() => Promise.resolve(() => undefined)),
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
    getSetupState.mockResolvedValue({
      hasCompleted: true,
      currentInvitation: null,
      deviceName: 'test',
    })
    issuePairingInvitation.mockResolvedValue({
      code: '123456789',
      expiresAtMs: Date.now() + 300_000,
    })
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
})
