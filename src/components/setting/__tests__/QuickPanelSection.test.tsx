import { openUrl } from '@tauri-apps/plugin-opener'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { getQuickPanelDoubleTapAvailability } from '@/api/tauri-command'
import QuickPanelSection from '@/components/setting/QuickPanelSection'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetting } from '@/hooks/useSetting'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType } from '@/types/setting'

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/api/tauri-command', () => ({
  getQuickPanelDoubleTapAvailability: vi.fn(),
}))

vi.mock('@/hooks/usePlatform', () => ({
  usePlatform: vi.fn(),
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: vi.fn(),
}))

vi.mock('@/lib/ipc', () => ({
  commands: { restartApp: vi.fn() },
}))

vi.mock('@/components/setting/ShortcutRow', () => ({
  ShortcutRow: () => <div>recorded-shortcut-row</div>,
}))

const mockOpenUrl = vi.mocked(openUrl)
const mockGetQuickPanelDoubleTapAvailability = vi.mocked(getQuickPanelDoubleTapAvailability)
const mockUsePlatform = vi.mocked(usePlatform)
const mockUseSetting = vi.mocked(useSetting)

beforeEach(() => {
  vi.clearAllMocks()
  mockGetQuickPanelDoubleTapAvailability.mockResolvedValue('supported')
  Object.defineProperties(HTMLElement.prototype, {
    hasPointerCapture: {
      configurable: true,
      value: vi.fn(() => false),
    },
    setPointerCapture: {
      configurable: true,
      value: vi.fn(),
    },
    releasePointerCapture: {
      configurable: true,
      value: vi.fn(),
    },
    scrollIntoView: {
      configurable: true,
      value: vi.fn(),
    },
  })
  mockUsePlatform.mockReturnValue({
    isWindows: false,
    isMac: true,
    isLinux: false,
    isTauri: true,
    reduceVisualEffects: false,
  })
})

function setup(keyboardShortcuts: Record<string, string | string[]> = {}) {
  const updateQuickPanelSetting = vi
    .fn<SettingContextType['updateQuickPanelSetting']>()
    .mockResolvedValue({ restartRequired: false })
  const updateKeyboardShortcuts = vi.fn()

  mockUseSetting.mockReturnValue({
    setting: makeBaseSettings({
      keyboardShortcuts,
      quickPanel: {
        enabled: true,
        position: 'center',
        doubleTapModifier: 'disabled',
      },
    }),
    loading: false,
    error: null,
    reloadSetting: vi.fn(),
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
    updateAutostart: vi.fn(),
    updateSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateKeyboardShortcuts,
    updateFileSyncSetting: vi.fn(),
    updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
    saveRelay: vi.fn().mockResolvedValue({
      restartRequired: false,
      credentialStatus: { configured: false },
    }),
    updateQuickPanelSetting,
  })

  render(<QuickPanelSection />)
  return { updateKeyboardShortcuts, updateQuickPanelSetting }
}

describe('QuickPanelSection modifier double-tap trigger', () => {
  it('disables modifier double-tap when the Linux session has no supported backend', async () => {
    mockUsePlatform.mockReturnValue({
      isWindows: false,
      isMac: false,
      isLinux: true,
      isTauri: true,
      reduceVisualEffects: true,
    })
    mockGetQuickPanelDoubleTapAvailability.mockResolvedValue('unsupported_display_session')
    setup()

    await waitFor(() => {
      expect(screen.getAllByRole('combobox')[1]).toBeDisabled()
    })
    expect(
      screen.getByText('settings.sections.quickPanel.doubleTap.unsupported')
    ).toBeInTheDocument()
    expect(screen.getByText('recorded-shortcut-row')).toBeInTheDocument()
  })

  it('shows an explicit macOS Accessibility action without prompting in the background', async () => {
    const user = userEvent.setup()
    mockGetQuickPanelDoubleTapAvailability.mockResolvedValue('accessibility_permission_required')
    setup()

    const button = await screen.findByRole('button', {
      name: 'settings.sections.quickPanel.doubleTap.openSettings',
    })
    expect(screen.getAllByRole('combobox')[1]).toBeDisabled()

    await user.click(button)
    expect(mockOpenUrl).toHaveBeenCalledWith(
      'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility'
    )
  })

  it('offers platform labels and keeps the recorded shortcut editor', async () => {
    const user = userEvent.setup()
    const { updateQuickPanelSetting } = setup()

    expect(screen.getByText('recorded-shortcut-row')).toBeInTheDocument()

    const selects = screen.getAllByRole('combobox')
    await user.click(selects[1]!)
    await user.click(
      await screen.findByRole('option', {
        name: 'settings.sections.quickPanel.doubleTap.command',
      })
    )

    await waitFor(() => {
      expect(updateQuickPanelSetting).toHaveBeenCalledWith({ doubleTapModifier: 'meta' })
    })
  })
})

describe('QuickPanelSection Windows Win+V shortcut', () => {
  it('shows the Win+V setting only on Windows', () => {
    setup()
    expect(
      screen.queryByRole('switch', {
        name: 'settings.sections.quickPanel.winV.label',
      })
    ).not.toBeInTheDocument()

    mockUsePlatform.mockReturnValue({
      isWindows: true,
      isMac: false,
      isLinux: false,
      isTauri: true,
      reduceVisualEffects: true,
    })
    setup()
    expect(
      screen.getByRole('switch', {
        name: 'settings.sections.quickPanel.winV.label',
      })
    ).toBeInTheDocument()
  })

  it('adds Win+V without replacing the recorded quick panel shortcut', async () => {
    const user = userEvent.setup()
    mockUsePlatform.mockReturnValue({
      isWindows: true,
      isMac: false,
      isLinux: false,
      isTauri: true,
      reduceVisualEffects: true,
    })
    const { updateKeyboardShortcuts } = setup({
      'global.toggleQuickPanel': 'ctrl+shift+space',
    })

    await user.click(
      screen.getByRole('switch', {
        name: 'settings.sections.quickPanel.winV.label',
      })
    )

    await waitFor(() => {
      expect(updateKeyboardShortcuts).toHaveBeenCalledWith(
        { 'global.toggleQuickPanel': 'ctrl+shift+space' },
        { 'global.toggleQuickPanel': ['ctrl+shift+space', 'meta+v'] }
      )
    })
  })

  it('removes only Win+V when disabled', async () => {
    const user = userEvent.setup()
    mockUsePlatform.mockReturnValue({
      isWindows: true,
      isMac: false,
      isLinux: false,
      isTauri: true,
      reduceVisualEffects: true,
    })
    const { updateKeyboardShortcuts } = setup({
      'global.toggleQuickPanel': ['ctrl+shift+space', 'meta+v'],
    })

    await user.click(
      screen.getByRole('switch', {
        name: 'settings.sections.quickPanel.winV.label',
      })
    )

    await waitFor(() => {
      expect(updateKeyboardShortcuts).toHaveBeenCalledWith(
        { 'global.toggleQuickPanel': ['ctrl+shift+space', 'meta+v'] },
        { 'global.toggleQuickPanel': 'ctrl+shift+space' }
      )
    })
  })
})
