import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import ConnectMobileDialog from '@/components/device/ConnectMobileDialog'
import i18n from '@/i18n'

const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  register: vi.fn(),
  getSetup: vi.fn(),
  issue: vi.fn(),
  cancel: vi.fn(),
  trust: vi.fn(),
  unlock: vi.fn(),
}))
vi.mock('@/api/tauri-command/mobile_sync', () => ({
  DEFAULT_MOBILE_LAN_PORT: 42720,
  getMobileSyncSettings: api.getSettings,
  updateMobileSyncSettings: api.updateSettings,
  registerMobileDevice: api.register,
  isMobileSyncError: (error: unknown) =>
    typeof error === 'object' && error !== null && 'code' in error,
}))
vi.mock('@/api/daemon/setupV2', () => ({
  getSetupState: api.getSetup,
  issuePairingInvitation: api.issue,
  cancelInvitation: api.cancel,
}))
vi.mock('@/api/daemon/device-trust', () => ({ getDeviceTrustSnapshot: api.trust }))
vi.mock('@/api/security', () => ({
  unlockSpaceWithPassphrase: api.unlock,
  isUnlockSpaceError: () => false,
}))
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: () => () => {},
    onReconnect: () => () => {},
  },
}))

const enabled = {
  enabled: true,
  lanListenEnabled: true,
  lanPort: 42720,
  shortcutInstallMethods: [],
}
const callbacks = () => ({
  onOpenChange: vi.fn(),
  onMobileSuccess: vi.fn(),
  onDirectSuccess: vi.fn(),
  onSettingsChange: vi.fn(),
  onConfigure: vi.fn(),
})

describe('ConnectMobileDialog', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('zh-CN')
  })
  beforeEach(() => {
    vi.clearAllMocks()
    api.getSettings.mockResolvedValue(enabled)
    api.getSetup.mockResolvedValue({ currentInvitation: null, rePairingRequired: false })
    api.trust.mockResolvedValue({
      localDeviceId: 'local',
      devices: [{ deviceId: 'local', membership: 'active' }],
    })
    api.issue.mockResolvedValue({ code: '012-345', expiresAtMs: Date.now() + 300000 })
    api.cancel.mockResolvedValue(undefined)
    api.updateSettings.mockResolvedValue({ lanListenerBindError: null })
    api.register.mockResolvedValue({ deviceId: 'phone', label: 'My phone' })
    api.unlock.mockResolvedValue({ spaceId: 'space' })
  })

  it('defaults to regular sync without enabling it or issuing an invitation', async () => {
    api.getSettings.mockResolvedValue({ ...enabled, enabled: false })
    render(<ConnectMobileDialog open {...callbacks()} />)
    expect(screen.getByRole('tab', { name: '常规同步' })).toHaveAttribute('aria-selected', 'true')
    expect(await screen.findByText(/42720/)).toBeVisible()
    expect(api.updateSettings).not.toHaveBeenCalled()
    expect(api.issue).not.toHaveBeenCalled()
    expect(screen.getByText('实验性')).toBeVisible()
  })

  it('preserves the form and invitation while switching methods', async () => {
    const user = userEvent.setup()
    render(<ConnectMobileDialog open {...callbacks()} />)
    const input = await screen.findByRole('textbox')
    await user.type(input, 'My phone')
    const footer = screen
      .getByRole('button', { name: i18n.t('devices.mobileSync.add.submit') })
      .closest('[data-slot="dialog-footer"]')
    expect(footer?.parentElement).toBe(screen.getByRole('dialog'))
    expect(footer?.closest('[role="tabpanel"]')).toBeNull()
    await user.click(screen.getByRole('tab', { name: /设备直连/ }))
    expect(await screen.findByTestId('add-device-invitation-code')).toBeVisible()
    expect(
      screen
        .getByRole('button', { name: i18n.t('devices.addDevice.actions.copy') })
        .closest('[data-slot="dialog-footer"]')
    ).toBe(footer)
    expect(
      screen.queryByRole('button', { name: i18n.t('devices.mobileSync.add.submit') })
    ).not.toBeInTheDocument()
    expect(screen.getAllByRole('dialog')).toHaveLength(1)
    await user.click(screen.getByRole('tab', { name: '常规同步' }))
    expect(screen.getByRole('textbox')).toHaveValue('My phone')
    await user.click(screen.getByRole('tab', { name: /设备直连/ }))
    expect(screen.getByTestId('add-device-invitation-code')).toBeVisible()
    expect(api.issue).toHaveBeenCalledTimes(1)
    expect(api.cancel).not.toHaveBeenCalled()
  })

  it('enables regular sync only after confirmation then registers the phone', async () => {
    const user = userEvent.setup()
    const props = callbacks()
    api.getSettings.mockResolvedValueOnce({ ...enabled, enabled: false })
    render(<ConnectMobileDialog open {...props} />)
    await user.click(
      await screen.findByRole('button', {
        name: i18n.t('devices.mobileSync.enableConfirm.confirm'),
      })
    )
    expect(api.updateSettings).toHaveBeenCalledWith({ enabled: true, lanListenEnabled: true })
    await user.type(await screen.findByRole('textbox'), 'My phone')
    await user.click(screen.getByRole('button', { name: i18n.t('devices.mobileSync.add.submit') }))
    await waitFor(() =>
      expect(props.onMobileSuccess).toHaveBeenCalledWith({ deviceId: 'phone', label: 'My phone' })
    )
    expect(props.onOpenChange).toHaveBeenCalledWith(false)
  })

  it('blocks registration after an enable failure but permits direct pairing', async () => {
    const user = userEvent.setup()
    api.getSettings.mockResolvedValue({ ...enabled, enabled: false })
    api.updateSettings.mockResolvedValue({ lanListenerBindError: 'port unavailable' })
    render(<ConnectMobileDialog open {...callbacks()} />)
    await user.click(
      await screen.findByRole('button', {
        name: i18n.t('devices.mobileSync.enableConfirm.confirm'),
      })
    )
    expect(await screen.findByRole('alert')).toHaveTextContent('port unavailable')
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    await user.click(screen.getByRole('tab', { name: /设备直连/ }))
    expect(await screen.findByTestId('add-device-invitation-code')).toBeVisible()
    expect(api.register).not.toHaveBeenCalled()
  })

  it('shows load errors inline and retries', async () => {
    const user = userEvent.setup()
    api.getSettings.mockRejectedValueOnce(new Error('offline'))
    render(<ConnectMobileDialog open {...callbacks()} />)
    expect(await screen.findByRole('alert')).toHaveTextContent(
      i18n.t('devices.connectMobile.loadFailed')
    )
    await user.click(screen.getByRole('button', { name: i18n.t('devices.list.actions.retry') }))
    expect(await screen.findByRole('textbox')).toBeVisible()
  })

  it('prevents tab switching and closing while registration is pending', async () => {
    const user = userEvent.setup()
    const props = callbacks()
    let finish!: (value: unknown) => void
    api.register.mockImplementation(
      () =>
        new Promise(resolve => {
          finish = resolve
        })
    )
    render(<ConnectMobileDialog open {...props} />)
    await user.type(await screen.findByRole('textbox'), 'My phone')
    await user.click(screen.getByRole('button', { name: i18n.t('devices.mobileSync.add.submit') }))
    expect(screen.getByRole('tab', { name: /设备直连/ })).toHaveAttribute('aria-disabled', 'true')
    fireEvent.click(screen.getByRole('tab', { name: /设备直连/ }))
    expect(screen.getByRole('tab', { name: '常规同步' })).toHaveAttribute('aria-selected', 'true')
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(props.onOpenChange).not.toHaveBeenCalled()
    await act(async () => {
      finish({ deviceId: 'phone' })
    })
    expect(props.onOpenChange).toHaveBeenCalledWith(false)
  })

  it('submits direct pairing credentials from the shared footer', async () => {
    const user = userEvent.setup()
    api.getSetup.mockResolvedValue({ currentInvitation: null, rePairingRequired: true })
    render(<ConnectMobileDialog open {...callbacks()} />)
    await screen.findByRole('textbox')
    await user.click(screen.getByRole('tab', { name: /设备直连/ }))
    const input = await screen.findByLabelText(
      i18n.t('devices.addDevice.rePairing.passphraseLabel')
    )
    const submit = screen.getByTestId('re-pairing-confirm-passphrase')
    expect(submit.closest('[data-slot="dialog-footer"]')?.parentElement).toBe(
      screen.getByRole('dialog')
    )
    await user.type(input, 'test passphrase')
    await user.click(submit)
    expect(await screen.findByTestId('add-device-invitation-code')).toBeVisible()
    expect(api.unlock).toHaveBeenCalledExactlyOnceWith('test passphrase')
  })
})
