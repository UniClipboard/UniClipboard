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

const setup = (
  theme: Settings['general']['theme'] = 'light',
  generalOverrides: Partial<Settings['general']> = {}
) => {
  const updateGeneralSetting = vi
    .fn<SettingContextType['updateGeneralSetting']>()
    .mockResolvedValue(undefined)

  mockUseSetting.mockReturnValue({
    setting: {
      ...baseSetting,
      general: {
        ...baseSetting.general,
        theme,
        ...generalOverrides,
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
    saveRelay: vi.fn().mockResolvedValue({
      restartRequired: false,
      credentialStatus: { configured: false },
    }),
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
    searchInTitleBar: true,
    useSystemWindowFrame: false,
    windowFramePreference: 'custom',
    setWindowFramePreference: vi.fn().mockResolvedValue(undefined),
  })
})

describe('AppearanceSection', () => {
  it('显示三种主题预览和明确的主题选择', () => {
    setup('system')

    expect(screen.queryByRole('switch')).toBeNull()
    expect(
      screen.getByRole('radio', {
        name: 'settings.sections.appearance.themePreview.followSystem',
      })
    ).toBeChecked()
    expect(document.querySelectorAll('[data-appearance-theme-preview]')).toHaveLength(3)
    expect(
      document.querySelectorAll('[data-appearance-theme-preview="system"] .appearance-theme-window')
    ).toHaveLength(2)
    expect(screen.queryByRole('heading', { level: 1 })).toBeNull()
    expect(screen.getByText('appearanceLayout.themeHelp.system')).toBeVisible()
    expect(document.querySelector('[data-testid="appearance-settings"] h3')).toBeNull()
  })

  it('选择跟随系统会保存系统模式', async () => {
    const user = userEvent.setup()
    const { updateGeneralSetting } = setup('light')

    await user.click(
      screen.getByRole('radio', {
        name: 'settings.sections.appearance.themePreview.followSystem',
      })
    )

    await waitFor(() => {
      expect(updateGeneralSetting).toHaveBeenCalledWith({ theme: 'system' })
    })
  })

  it('预览分别反映浅色与深色的自定义强调色', () => {
    setup('system', {
      themeOverridesLight: { primary: '#ff0000' },
      themeOverridesDark: { primary: '#0000ff' },
    })
    expect(
      document.querySelector(
        '[data-appearance-theme-preview="light"] .appearance-theme-window-content > span'
      )
    ).toHaveStyle({ backgroundColor: '#ff0000' })
    expect(
      document.querySelector(
        '[data-appearance-theme-preview="dark"] .appearance-theme-window-content > span'
      )
    ).toHaveStyle({ backgroundColor: '#0000ff' })
    expect(
      document.querySelector(
        '[data-appearance-theme-preview="system"] .appearance-theme-window:last-child .appearance-theme-window-content > span'
      )
    ).toHaveStyle({ backgroundColor: '#0000ff' })
  })

  it('自定义颜色默认收起，选择色块只更新对应主题', async () => {
    const user = userEvent.setup()
    const { updateGeneralSetting } = setup()
    expect(document.querySelector('details')).not.toHaveAttribute('open')
    await user.click(screen.getByRole('combobox', { name: 'appearanceLayout.darkPalette' }))
    await user.click(await screen.findByRole('option', { name: 'blue' }))
    await waitFor(() =>
      expect(updateGeneralSetting).toHaveBeenCalledWith({
        themeColorDark: 'blue',
        themeColor: null,
      })
    )
    await user.click(screen.getByText('appearanceLayout.customColors'))
    expect(document.querySelector('details')).toHaveAttribute('open')
    expect(
      screen.getByRole('button', { name: 'settings.sections.appearance.lightTheme.accent' })
    ).toBeVisible()
  })

  it.each(['auto', 'custom', 'system', 'none'] as const)(
    '允许选择标题栏模式 %s',
    async preference => {
      const user = userEvent.setup()
      const setWindowFramePreference = vi.fn().mockResolvedValue(undefined)
      mockUseWindowFrame.mockReturnValue({
        canChooseSystemFrame: true,
        hasCustomTitleBar: true,
        hasCustomWindowControls: true,
        searchInTitleBar: true,
        useSystemWindowFrame: false,
        windowFramePreference: preference === 'custom' ? 'system' : 'custom',
        setWindowFramePreference,
      })

      setup()
      const selector = screen.getByRole('combobox', {
        name: 'settings.sections.appearance.windowFrame.title',
      })

      expect(selector).toBeVisible()
      await user.click(selector)
      await user.click(
        await screen.findByRole('option', {
          name: `settings.sections.appearance.windowFrame.${preference}`,
        })
      )

      expect(setWindowFramePreference).toHaveBeenCalledWith(preference)
    }
  )
})
