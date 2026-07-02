import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { fetchSponsors } from '@/api/sponsors'
import AboutSection from '@/components/setting/AboutSection'
import { SettingContext } from '@/contexts/setting-context'
import { UpdateContext } from '@/contexts/update-context'
import type { UpdateContextType } from '@/contexts/update-context'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType, Settings } from '@/types/setting'

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.4.0-alpha.6'),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/hooks/useShortcutLayer', () => ({
  useShortcutLayer: vi.fn(),
}))

vi.mock('@/api/sponsors', () => ({
  fetchSponsors: vi.fn(() => new Promise(() => undefined)),
}))

const mockFetchSponsors = vi.mocked(fetchSponsors)

beforeAll(() => {
  if (!HTMLElement.prototype.hasPointerCapture) {
    Object.defineProperty(HTMLElement.prototype, 'hasPointerCapture', {
      value: () => false,
    })
  }
  if (!HTMLElement.prototype.setPointerCapture) {
    Object.defineProperty(HTMLElement.prototype, 'setPointerCapture', {
      value: () => undefined,
    })
  }
  if (!HTMLElement.prototype.releasePointerCapture) {
    Object.defineProperty(HTMLElement.prototype, 'releasePointerCapture', {
      value: () => undefined,
    })
  }
  if (!HTMLElement.prototype.scrollIntoView) {
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      value: () => undefined,
    })
  }
})

const baseSetting: Settings = makeBaseSettings()

interface RenderAboutSectionOptions {
  setting?: Settings
  updateGeneralSetting?: SettingContextType['updateGeneralSetting']
  checkForUpdates?: UpdateContextType['checkForUpdates']
  isCheckingUpdate?: boolean
}

function renderAboutSection({
  setting = baseSetting,
  updateGeneralSetting = vi
    .fn<SettingContextType['updateGeneralSetting']>()
    .mockResolvedValue(undefined),
  checkForUpdates = vi.fn<UpdateContextType['checkForUpdates']>().mockResolvedValue(null),
  isCheckingUpdate = false,
}: RenderAboutSectionOptions = {}) {
  const user = userEvent.setup()

  const view = render(
    <SettingContext.Provider
      value={{
        setting,
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
      }}
    >
      <UpdateContext.Provider
        value={{
          state: { phase: 'idle', info: null, downloaded: 0, total: null },
          updateInfo: null,
          isCheckingUpdate,
          checkForUpdates,
          downloadUpdate: vi.fn().mockResolvedValue(undefined),
          cancelDownload: vi.fn().mockResolvedValue(undefined),
          installUpdate: vi.fn(),
          downloadProgress: { downloaded: 0, total: null, phase: 'idle' as const },
          installKind: 'macos',
          isSystemManaged: false,
          isManualUpdate: false,
        }}
      >
        <AboutSection />
      </UpdateContext.Provider>
    </SettingContext.Provider>
  )

  return {
    ...view,
    user,
    updateGeneralSetting,
    checkForUpdates,
  }
}

describe('AboutSection', () => {
  it('runs update check when clicking the button', async () => {
    const checkForUpdates = vi.fn().mockResolvedValue({
      version: '0.1.1',
      currentVersion: '0.1.0',
      date: '2026-01-25T00:00:00Z',
      body: 'Bug fixes',
    })

    const { user } = renderAboutSection({ checkForUpdates })

    await user.click(screen.getByRole('button', { name: 'settings.sections.about.checkUpdate' }))

    await waitFor(() => {
      expect(checkForUpdates).toHaveBeenCalledTimes(1)
    })

    expect(screen.getByText('update.title')).toBeInTheDocument()
  })

  it('toggles auto update checks', async () => {
    const updateGeneralSetting = vi.fn().mockResolvedValue(undefined)

    const { user } = renderAboutSection({ updateGeneralSetting })

    expect(screen.getByText('settings.sections.about.autoCheckUpdate.label')).toBeInTheDocument()

    // Two switches now (auto-check + auto-download); auto-check is rendered first.
    const [autoCheckSwitch] = screen.getAllByRole('switch')
    await user.click(autoCheckSwitch)

    await waitFor(() => {
      expect(updateGeneralSetting).toHaveBeenCalledWith({ autoCheckUpdate: false })
    })
  })

  it('toggles background download when auto-check is enabled', async () => {
    const updateGeneralSetting = vi.fn().mockResolvedValue(undefined)

    const { user } = renderAboutSection({ updateGeneralSetting })

    expect(screen.getByText('settings.sections.about.autoDownloadUpdate.label')).toBeInTheDocument()

    const [, autoDownloadSwitch] = screen.getAllByRole('switch')
    await user.click(autoDownloadSwitch)

    await waitFor(() => {
      expect(updateGeneralSetting).toHaveBeenCalledWith({ autoDownloadUpdate: true })
    })
  })

  it('disables background download switch when auto-check is off', () => {
    renderAboutSection({
      setting: {
        ...baseSetting,
        general: { ...baseSetting.general, autoCheckUpdate: false, autoDownloadUpdate: true },
      },
    })

    const [autoCheckSwitch, autoDownloadSwitch] = screen.getAllByRole('switch')
    expect(autoCheckSwitch).not.toBeDisabled()
    expect(autoDownloadSwitch).toBeDisabled()
    // Persisted preference is preserved but rendered as off, since check is gating it.
    expect(autoDownloadSwitch).toHaveAttribute('aria-checked', 'false')
    expect(
      screen.getByText('settings.sections.about.autoDownloadUpdate.disabledHint')
    ).toBeInTheDocument()
  })

  it('shows loading feedback while checking for updates', () => {
    const { container } = renderAboutSection({ isCheckingUpdate: true })

    const checkButton = screen.getByRole('button', {
      name: 'settings.sections.about.checkingUpdate',
    })

    expect(checkButton).toBeDisabled()
    expect(checkButton).toHaveAttribute('aria-busy', 'true')
    expect(checkButton).toHaveClass('w-44')
    expect(checkButton).toHaveClass('transition-colors')
    expect(checkButton).not.toHaveClass('transition-all')
    expect(checkButton).toHaveTextContent('settings.sections.about.checkingUpdate')
    expect(checkButton).not.toHaveTextContent('settings.sections.about.checkUpdate')
    expect(container.querySelector('.animate-spin')).toBeTruthy()
  })

  it('shows sponsor and GitHub star actions in the sponsors group', async () => {
    mockFetchSponsors.mockResolvedValueOnce([
      {
        id: 'sponsor-1',
        name: 'Ada Lovelace',
        tier: 'regular',
      },
    ])

    renderAboutSection()

    const sponsorLink = await screen.findByRole('link', {
      name: 'settings.sections.about.sponsors.becomeSponsor',
    })
    const starLink = screen.getByRole('link', {
      name: 'settings.sections.about.sponsors.githubStar',
    })

    expect(sponsorLink).toHaveAttribute('href', 'https://afdian.com/a/mkdir700')
    expect(starLink).toHaveAttribute('href', 'https://github.com/UniClipboard/UniClipboard')
  })

  it('checks the newly selected channel immediately after saving it', async () => {
    const updateGeneralSetting = vi.fn().mockResolvedValue(undefined)
    const checkForUpdates = vi.fn().mockResolvedValue(null)
    const { user } = renderAboutSection({ updateGeneralSetting, checkForUpdates })

    await user.click(screen.getByRole('combobox'))
    await user.click(
      await screen.findByRole('option', { name: 'settings.sections.about.updateChannel.stable' })
    )

    await waitFor(() => {
      expect(updateGeneralSetting).toHaveBeenCalledWith({ updateChannel: 'stable' })
    })
    expect(checkForUpdates).toHaveBeenCalledWith('stable')
  })

  it('warns before switching to alpha and checks alpha after confirmation', async () => {
    const updateGeneralSetting = vi.fn().mockResolvedValue(undefined)
    const checkForUpdates = vi.fn().mockResolvedValue(null)
    const { user } = renderAboutSection({ updateGeneralSetting, checkForUpdates })

    await user.click(screen.getByRole('combobox'))
    await user.click(
      await screen.findByRole('option', { name: 'settings.sections.about.updateChannel.alpha' })
    )

    expect(
      await screen.findByText('settings.sections.about.updateChannel.alphaWarning.title')
    ).toBeInTheDocument()
    expect(updateGeneralSetting).not.toHaveBeenCalled()

    await user.click(
      screen.getByRole('button', {
        name: 'settings.sections.about.updateChannel.alphaWarning.confirm',
      })
    )

    await waitFor(() => {
      expect(updateGeneralSetting).toHaveBeenCalledWith({ updateChannel: 'alpha' })
    })
    expect(checkForUpdates).toHaveBeenCalledWith('alpha')
  })
})
