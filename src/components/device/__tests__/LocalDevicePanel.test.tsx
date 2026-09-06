import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import LocalDevicePanel from '@/components/device/LocalDevicePanel'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({
  updateSyncSetting: vi.fn(),
  updateFileSyncSetting: vi.fn(),
  toastError: vi.fn(),
  setting: { sync: { syncEnabled: true }, fileSync: { fileSyncEnabled: true } } as object | null,
}))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: async () => '1.0.0' }))
vi.mock('@/hooks/useSetting', () => ({
  useSettingSelector: (selector: (context: typeof mocks) => unknown) => selector(mocks),
}))
vi.mock('@/components/device/SwitchSpaceDialog', () => ({ default: () => null }))
vi.mock('@/components/ui/toast', () => ({ toast: { error: mocks.toastError } }))

function renderPanel() {
  return render(
    <LocalDevicePanel localDevice={{ deviceName: 'My Mac', peerId: 'local' }} memberCount={2} />
  )
}

describe('local device settings', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    vi.clearAllMocks()
    mocks.setting = { sync: { syncEnabled: true }, fileSync: { fileSyncEnabled: true } }
    mocks.updateSyncSetting.mockResolvedValue(undefined)
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
