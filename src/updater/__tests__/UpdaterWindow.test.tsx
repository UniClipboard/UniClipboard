import { openUrl } from '@tauri-apps/plugin-opener'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import {
  checkForUpdate,
  confirmUpdate,
  downloadUpdate,
  ensureUpdateAuthorized,
  getAutoDownloadUpdate,
  getDownloadProgress,
  getInstallKind,
  installUpdate,
  subscribeUpdateAvailable,
  subscribeUpdateProgress,
} from '@/api/updater'
import UpdaterWindow from '@/updater/UpdaterWindow'

const closeWindow = vi.fn().mockResolvedValue(undefined)

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ close: closeWindow }),
}))

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/hooks/useThemeSync', () => ({
  useThemeSync: () => {},
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('@/api/updater', () => ({
  checkForUpdate: vi.fn(),
  confirmUpdate: vi.fn(),
  downloadUpdate: vi.fn().mockResolvedValue(undefined),
  ensureUpdateAuthorized: vi.fn().mockResolvedValue(undefined),
  cancelDownload: vi.fn().mockResolvedValue(undefined),
  installUpdate: vi.fn().mockResolvedValue(undefined),
  getInstallKind: vi.fn().mockResolvedValue('macos'),
  getAutoDownloadUpdate: vi.fn().mockResolvedValue(true),
  setAutoDownloadUpdate: vi.fn().mockResolvedValue(undefined),
  skipVersion: vi.fn().mockResolvedValue(undefined),
  subscribeUpdateAvailable: vi.fn().mockResolvedValue(() => {}),
  subscribeUpdateProgress: vi.fn().mockResolvedValue(() => {}),
  getDownloadProgress: vi.fn(),
}))

const META = {
  version: '1.2.0',
  currentVersion: '1.1.0',
  body: 'notes',
  date: null,
  confirmation: { status: 'not_required' },
}

function snapshot(overrides: Record<string, unknown> = {}) {
  return {
    phase: 'available',
    downloaded: 0,
    total: null,
    version: META.version,
    currentVersion: META.currentVersion,
    body: META.body,
    date: null,
    confirmation: META.confirmation,
    ...overrides,
  } as Awaited<ReturnType<typeof getDownloadProgress>>
}

function mockSnapshot(overrides: Record<string, unknown> = {}) {
  vi.mocked(getDownloadProgress).mockResolvedValue(snapshot(overrides))
}

beforeEach(() => {
  // clearAllMocks keeps implementations; re-arm them explicitly so a
  // per-test mockRejectedValue cannot leak into later tests.
  vi.clearAllMocks()
  vi.mocked(getInstallKind).mockResolvedValue('macos')
  vi.mocked(getAutoDownloadUpdate).mockResolvedValue(true)
  vi.mocked(downloadUpdate).mockResolvedValue(undefined)
  vi.mocked(ensureUpdateAuthorized).mockResolvedValue(undefined)
  vi.mocked(subscribeUpdateProgress).mockResolvedValue(() => {})
  vi.mocked(subscribeUpdateAvailable).mockResolvedValue(() => {})
})

describe('UpdaterWindow', () => {
  it('available: primary button downloads (recoverable path), not inline install', async () => {
    mockSnapshot({ phase: 'available' })
    vi.mocked(checkForUpdate).mockResolvedValue(META as never)
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    const btn = await screen.findByText('updater.window.downloadUpdate')
    await user.click(btn)

    await waitFor(() => expect(downloadUpdate).toHaveBeenCalledTimes(1))
    expect(installUpdate).not.toHaveBeenCalled()
  })

  it('opens a full-window confirmation before continuing the requested download', async () => {
    const pending = {
      ...META,
      confirmation: { status: 'pending', description: '## Important\n\nReview this first.' },
    } as const
    const confirmed = {
      ...META,
      confirmation: { status: 'confirmed', description: '## Important\n\nReview this first.' },
    } as const
    vi.mocked(getDownloadProgress).mockResolvedValue(
      snapshot({ confirmation: pending.confirmation })
    )
    vi.mocked(confirmUpdate).mockResolvedValue(confirmed as never)
    vi.mocked(checkForUpdate).mockResolvedValue(confirmed as never)
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    const download = await screen.findByText('updater.window.downloadUpdate')
    expect(download).toBeEnabled()
    expect(screen.queryByRole('heading', { name: 'Important' })).not.toBeInTheDocument()

    await user.click(download)
    const confirmationDialog = screen.getByRole('alertdialog')
    expect(confirmationDialog).toHaveClass('h-screen', 'w-screen')
    expect(screen.getByRole('heading', { name: 'Important' })).toBeInTheDocument()
    expect(downloadUpdate).not.toHaveBeenCalled()

    await user.click(screen.getByText('update.confirmation.acknowledge'))
    await waitFor(() => expect(confirmUpdate).toHaveBeenCalledWith(META.version))
    await waitFor(() => expect(downloadUpdate).toHaveBeenCalledTimes(1))
  })

  it('blocks an update whose required explanation is unavailable', async () => {
    mockSnapshot({ confirmation: { status: 'blocked' } })
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    const download = await screen.findByText('updater.window.downloadUpdate')
    expect(download).toBeEnabled()
    await user.click(download)

    expect(screen.getByText('update.confirmation.unavailable')).toBeInTheDocument()
    expect(screen.queryByText('update.confirmation.acknowledge')).not.toBeInTheDocument()
    expect(downloadUpdate).not.toHaveBeenCalled()
  })

  it('downloading: shows a background button that closes the window without cancelling', async () => {
    mockSnapshot({ phase: 'downloading', downloaded: 10, total: 100 })
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    const bg = await screen.findByText('update.downloadInBackground')
    await user.click(bg)

    await waitFor(() => expect(closeWindow).toHaveBeenCalledTimes(1))
    expect(installUpdate).not.toHaveBeenCalled()
    expect(downloadUpdate).not.toHaveBeenCalled()
  })

  it('ready: primary button installs cached bytes (no re-download)', async () => {
    mockSnapshot({ phase: 'ready', downloaded: 100, total: 100 })
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    const btn = await screen.findByText('update.installNow')
    await user.click(btn)

    await waitFor(() => expect(installUpdate).toHaveBeenCalledTimes(1))
    expect(downloadUpdate).not.toHaveBeenCalled()
  })

  it('available: re-check superseding to a newer version aborts the download', async () => {
    mockSnapshot({ phase: 'available' })
    vi.mocked(checkForUpdate).mockResolvedValue({
      ...META,
      version: '1.3.0',
    } as never)
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    const btn = await screen.findByText('updater.window.downloadUpdate')
    await user.click(btn)

    await waitFor(() => expect(checkForUpdate).toHaveBeenCalled())
    expect(downloadUpdate).not.toHaveBeenCalled()
  })

  it('portable build: primary button opens the release page instead of downloading', async () => {
    vi.mocked(getInstallKind).mockResolvedValue('windowsportable')
    mockSnapshot({ phase: 'available' })
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    const btn = await screen.findByText('update.packageManager.openReleasePage')
    await user.click(btn)

    await waitFor(() => expect(openUrl).toHaveBeenCalledTimes(1))
    expect(ensureUpdateAuthorized).toHaveBeenCalledWith(META.version)
    expect(downloadUpdate).not.toHaveBeenCalled()
    expect(installUpdate).not.toHaveBeenCalled()
  })

  it('download rejection (already ready): re-syncs to the ready view instead of a stale spinner', async () => {
    // Mount sees `available`; the download command then rejects with a
    // precondition error (no `Failed` broadcast), and the re-sync snapshot
    // reports the backend actually holds downloaded bytes.
    vi.mocked(getDownloadProgress)
      .mockResolvedValueOnce(snapshot({ phase: 'available' }))
      .mockResolvedValue(snapshot({ phase: 'ready', downloaded: 100, total: 100 }))
    vi.mocked(checkForUpdate).mockResolvedValue(META as never)
    vi.mocked(downloadUpdate).mockRejectedValue(
      new Error('updater: already downloaded, ready to install')
    )
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    await user.click(await screen.findByText('updater.window.downloadUpdate'))

    await screen.findByText('update.installNow')
    expect(getDownloadProgress).toHaveBeenCalledTimes(2)
  })

  it('download rejection (pending cleared): re-syncs to up-to-date, clearing stale info', async () => {
    // The re-sync snapshot has no version: the backend cleared the pending
    // update. `info` must be cleared too so the up-to-date view renders,
    // instead of re-offering a version that no longer exists.
    vi.mocked(getDownloadProgress)
      .mockResolvedValueOnce(snapshot({ phase: 'available' }))
      .mockResolvedValue(
        snapshot({ phase: 'idle', version: null, currentVersion: null, body: null })
      )
    vi.mocked(checkForUpdate).mockResolvedValue(META as never)
    vi.mocked(downloadUpdate).mockRejectedValue(new Error('updater: no pending update to download'))
    const user = userEvent.setup()
    render(<UpdaterWindow />)

    await user.click(await screen.findByText('updater.window.downloadUpdate'))

    await screen.findByText('updater.window.upToDateTitle')
    expect(screen.getByText('updater.window.close')).toBeInTheDocument()
  })
})
