import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import AppearanceSection from '@/components/setting/AppearanceSection'
import { useSetting } from '@/hooks/useSetting'
import { useUiScale } from '@/hooks/useUiScale'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType, Settings } from '@/types/setting'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: vi.fn(),
}))

vi.mock('@/hooks/useUiScale', () => ({
  useUiScale: vi.fn(),
}))

vi.mock('@/hooks/useWindowFrame', () => ({
  useWindowFrame: vi.fn(),
}))

vi.mock('@/lib/theme-transition', () => ({
  setTransitionOrigin: vi.fn(),
}))

const mockUseSetting = vi.mocked(useSetting)
const mockUseUiScale = vi.mocked(useUiScale)
const mockUseWindowFrame = vi.mocked(useWindowFrame)

const baseSetting: Settings = makeBaseSettings({
  general: { theme: 'light', themeColor: 'zinc', language: 'zh-CN' },
})

const setup = (theme: Settings['general']['theme'] = 'light') => {
  const updateGeneralSetting = vi
    .fn<SettingContextType['updateGeneralSetting']>()
    .mockResolvedValue(undefined)

  mockUseSetting.mockReturnValue({
    setting: {
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme,
      },
    },
    loading: false,
    error: null,
    reloadSetting: vi.fn(),
    updateSetting: vi.fn(),
    updateGeneralSetting,
    updateAutostart: vi.fn(),
    updateSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateKeyboardShortcuts: vi.fn(),
    updateFileSyncSetting: vi.fn(),
    updateNetworkSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
    updateQuickPanelSetting: vi.fn().mockResolvedValue({ restartRequired: false }),
  })

  mockUseUiScale.mockReturnValue({
    scale: 1,
    scalePercent: '100%',
    options: [{ value: 1, label: '100%' }],
    setScale: vi.fn(),
    resetScale: vi.fn(),
    isDefault: true,
    isSelected: option => option.value === 1,
    zoomIn: vi.fn(),
    zoomOut: vi.fn(),
    canZoomIn: false,
    canZoomOut: false,
  })

  render(<AppearanceSection />)

  return { updateGeneralSetting }
}

beforeEach(() => {
  vi.clearAllMocks()
  mockUseWindowFrame.mockReturnValue({
    canChooseSystemFrame: false,
    hasCustomTitleBar: true,
    hasCustomWindowControls: false,
    hasRoundedWindow: false,
    searchInTitleBar: true,
    useSystemWindowFrame: false,
    setUseSystemWindowFrame: vi.fn().mockResolvedValue(undefined),
  })
})

describe('AppearanceSection', () => {
  it('把跟随系统显示为主题卡片而不是开关', () => {
    setup('system')

    expect(screen.queryByRole('switch')).toBeNull()
    expect(
      screen.getByRole('button', {
        name: 'settings.sections.appearance.themePreview.followSystem',
      })
    ).toHaveAttribute('aria-pressed', 'true')
  })

  it('点击跟随系统卡片会切换到系统模式', async () => {
    const user = userEvent.setup()
    const { updateGeneralSetting } = setup('light')

    await user.click(
      screen.getByRole('button', {
        name: 'settings.sections.appearance.themePreview.followSystem',
      })
    )

    await waitFor(() => {
      expect(updateGeneralSetting).toHaveBeenCalledWith({ theme: 'system' })
    })
  })

  it('在支持的平台上允许启用系统窗口框', async () => {
    const user = userEvent.setup()
    const setUseSystemWindowFrame = vi.fn().mockResolvedValue(undefined)
    mockUseWindowFrame.mockReturnValue({
      canChooseSystemFrame: true,
      hasCustomTitleBar: true,
      hasCustomWindowControls: true,
      hasRoundedWindow: true,
      searchInTitleBar: true,
      useSystemWindowFrame: false,
      setUseSystemWindowFrame,
    })

    setup()
    const toggle = screen.getByRole('switch', {
      name: 'settings.sections.appearance.windowFrame.useSystem',
    })

    expect(toggle).toHaveAttribute('aria-checked', 'false')
    await user.click(toggle)

    expect(setUseSystemWindowFrame).toHaveBeenCalledWith(true)
  })
})
