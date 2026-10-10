import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { resetSetup } from '@/api/daemon/setupV2'
import { resetSpace } from '@/api/security'
import LocalDevicePanel from '@/components/device/LocalDevicePanel'
import i18n from '@/i18n'
import { refreshSetupState } from '@/store/setupRealtimeStore'

const mocks = vi.hoisted(() => ({
  updateSyncSetting: vi.fn(),
  updateFileSyncSetting: vi.fn(),
  toastError: vi.fn(),
  setting: { sync: { syncEnabled: true }, fileSync: { fileSyncEnabled: true } } as object | null,
}))
vi.mock('@/api/daemon/setupV2', () => ({ resetSetup: vi.fn() }))
vi.mock('@/api/security', () => ({
  resetSpace: vi.fn(),
  isFactoryResetError: (error: { code?: string }) => Boolean(error.code),
}))
vi.mock('@/store/setupRealtimeStore', () => ({ refreshSetupState: vi.fn() }))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: async () => '1.0.0' }))
vi.mock('@/hooks/useSetting', () => ({
  useSettingSelector: (selector: (context: typeof mocks) => unknown) => selector(mocks),
}))
vi.mock('@/components/device/SwitchSpaceDialog', () => ({ default: () => null }))
vi.mock('@/components/ui/toast', () => ({ toast: { error: mocks.toastError } }))

function renderPanel(onRebuildSucceeded?: () => void) {
  return render(
    <LocalDevicePanel
      onRebuildSucceeded={onRebuildSucceeded}
      localDevice={{ deviceName: 'My Mac', peerId: 'local' }}
      memberCount={2}
    />
  )
}

describe('local device settings', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    vi.clearAllMocks()
    mocks.setting = { sync: { syncEnabled: true }, fileSync: { fileSyncEnabled: true } }
    mocks.updateSyncSetting.mockResolvedValue(undefined)
  })
  it('requires explicit confirmation and cancels without resetting', async () => {
    const user = userEvent.setup()
    renderPanel()
    await user.click(screen.getByRole('button', { name: i18n.t('devices.panel.danger.reset') }))
    const confirm = screen.getByRole('button', {
      name: i18n.t('devices.panel.danger.modal.confirm'),
    })
    expect(confirm).toBeDisabled()
    await user.type(
      screen.getByLabelText(i18n.t('devices.panel.danger.modal.confirmPrompt')),
      'reset'
    )
    expect(confirm).toBeEnabled()
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.panel.danger.modal.cancel') })
    )
    expect(resetSpace).not.toHaveBeenCalled()
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
  })
  it('resets once and refreshes setup after confirmation', async () => {
    const user = userEvent.setup()
    vi.mocked(resetSetup).mockResolvedValue(undefined)
    renderPanel()
    await user.click(screen.getByRole('button', { name: i18n.t('devices.panel.danger.reset') }))
    await user.type(
      screen.getByLabelText(i18n.t('devices.panel.danger.modal.confirmPrompt')),
      'RESET'
    )
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.panel.danger.modal.confirm') })
    )
    await waitFor(() => expect(refreshSetupState).toHaveBeenCalledTimes(1))
    expect(resetSetup).toHaveBeenCalledTimes(1)
    expect(resetSpace).not.toHaveBeenCalled()
  })
  it('prevents repeated submissions and closing while reset is running', async () => {
    let finish!: () => void
    vi.mocked(resetSetup).mockReturnValue(
      new Promise<void>(resolve => {
        finish = resolve
      })
    )
    const onRebuildSucceeded = vi.fn()
    const user = userEvent.setup()
    renderPanel(onRebuildSucceeded)
    await user.click(screen.getByRole('button', { name: i18n.t('devices.panel.danger.reset') }))
    await user.type(
      screen.getByLabelText(i18n.t('devices.panel.danger.modal.confirmPrompt')),
      'RESET'
    )
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.panel.danger.modal.confirm') })
    )
    expect(
      screen.getByRole('button', { name: i18n.t('devices.panel.danger.modal.resetting') })
    ).toBeDisabled()
    expect(
      screen.getByRole('button', { name: i18n.t('devices.panel.danger.modal.cancel') })
    ).toBeDisabled()
    await user.keyboard('{Escape}')
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    expect(resetSetup).toHaveBeenCalledTimes(1)
    expect(resetSpace).not.toHaveBeenCalled()
    finish()
    await waitFor(() => expect(onRebuildSucceeded).toHaveBeenCalledTimes(1))
  })
  it('shows failure without leaving the device page and allows retry', async () => {
    vi.mocked(resetSetup)
      .mockRejectedValueOnce(new Error('unavailable'))
      .mockResolvedValue(undefined)
    const onRebuildSucceeded = vi.fn()
    const user = userEvent.setup()
    renderPanel(onRebuildSucceeded)
    await user.click(screen.getByRole('button', { name: i18n.t('devices.panel.danger.reset') }))
    await user.type(
      screen.getByLabelText(i18n.t('devices.panel.danger.modal.confirmPrompt')),
      'RESET'
    )
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.panel.danger.modal.confirm') })
    )
    expect(await screen.findByRole('alert')).toHaveTextContent(
      i18n.t('devices.panel.danger.failed')
    )
    expect(onRebuildSucceeded).not.toHaveBeenCalled()
    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.panel.danger.modal.confirm') })
    )
    await waitFor(() => expect(onRebuildSucceeded).toHaveBeenCalledTimes(1))
  })
  it('labels the switches and prevents repeat changes while saving', async () => {
    let finish!: () => void
    mocks.updateSyncSetting.mockReturnValue(
      new Promise<void>(resolve => {
        finish = resolve
      })
    )
    const user = userEvent.setup()
    renderPanel()
    const control = screen.getByRole('switch', {
      name: i18n.t('devices.panel.policies.syncEnabled.title'),
    })
    await user.click(control)
    expect(control).toBeDisabled()
    expect(control).toHaveAttribute('aria-busy', 'true')
    await user.click(control)
    expect(mocks.updateSyncSetting).toHaveBeenCalledTimes(1)
    expect(mocks.updateSyncSetting).toHaveBeenCalledWith({ syncEnabled: false })
    finish()
    await waitFor(() => expect(control).toBeEnabled())
    expect(control).toHaveAttribute('aria-busy', 'false')
  })
  it('reports a failed save and retains the last saved setting', async () => {
    mocks.updateSyncSetting.mockRejectedValue(new Error('offline'))
    const user = userEvent.setup()
    renderPanel()
    await user.click(screen.getAllByRole('switch')[0])
    await waitFor(() =>
      expect(mocks.toastError).toHaveBeenCalledWith(i18n.t('devices.settings.sync.updateFailed'))
    )
    expect(screen.getAllByRole('switch')[0]).toBeChecked()
  })
  it('does not offer enabled controls before settings are available', () => {
    mocks.setting = null
    renderPanel()
    for (const control of screen.getAllByRole('switch')) expect(control).toBeDisabled()
    expect(screen.queryByText(i18n.t('devices.thisDevice.syncActive'))).not.toBeInTheDocument()
  })
  it('does not disable the sync switch while saving only file sync', async () => {
    let finish!: () => void
    mocks.updateFileSyncSetting.mockReturnValue(
      new Promise<void>(resolve => {
        finish = resolve
      })
    )
    const user = userEvent.setup()
    renderPanel()
    const [sync, file] = screen.getAllByRole('switch')
    await user.click(file)
    expect(file).toBeDisabled()
    expect(sync).toBeEnabled()
    finish()
    await waitFor(() => expect(file).toBeEnabled())
  })
})
