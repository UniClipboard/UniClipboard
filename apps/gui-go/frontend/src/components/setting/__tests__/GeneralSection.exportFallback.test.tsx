import '@testing-library/jest-dom/vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { exportLogs, getDiagnosticCaptureStatus } from '@/api/daemon/diagnostics'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import GeneralSection from '@/components/setting/GeneralSection'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { makeBaseSettings } from '@/test/fixtures/settings'

// Echo the key plus interpolation options so the rendered text is assertable.
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}|${JSON.stringify(options)}` : key,
  }),
}))

vi.mock('@/api/daemon/diagnostics', () => ({
  updateDebugMode: vi.fn(),
  exportLogs: vi.fn(),
  getDiagnosticCaptureStatus: vi.fn(),
  getNetworkLogStatus: vi.fn(),
  startDiagnosticCapture: vi.fn(),
  startNetworkCapture: vi.fn(),
  stopDiagnosticCapture: vi.fn(),
  stopNetworkCapture: vi.fn(),
}))
vi.mock('@/api/storage', () => ({ openLogsDirectory: vi.fn(), revealPath: vi.fn() }))
vi.mock('@/hooks/useSetting', () => ({ useSetting: vi.fn() }))
vi.mock('@/lib/ipc', () => ({
  commands: { restartDaemon: vi.fn(), restartApp: vi.fn(), exportStartupLogs: vi.fn() },
}))
vi.mock('@/components/ui/toast', () => ({
  toast: { success: vi.fn(), error: vi.fn(), message: vi.fn() },
}))

const EXPORT = 'settings.sections.general.logs.export'

function daemonExportFailure(cause: string) {
  return new DaemonApiError(
    DaemonErrorCode.INTERNAL_ERROR,
    '500 on /diagnostics/export',
    { code: 'diagnostic_export_failed', message: 'failed', details: { cause } },
    500
  )
}

async function clickExport() {
  const user = userEvent.setup()
  render(<GeneralSection />)
  await user.click(screen.getByRole('button', { name: `${EXPORT}.button` }))
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(useSetting).mockReturnValue({
    setting: makeBaseSettings(),
    loading: false,
    reloadSetting: vi.fn().mockResolvedValue(undefined),
  } as unknown as ReturnType<typeof useSetting>)
  vi.mocked(getDiagnosticCaptureStatus).mockRejectedValue(new Error('unused'))
})

describe('offline log export fallback notice', () => {
  it('lists the sources the offline export lacks', async () => {
    vi.mocked(exportLogs).mockRejectedValue(daemonExportFailure('source_read_failed'))
    vi.mocked(commands.exportStartupLogs).mockResolvedValue({
      path: '/dl/offline.zip',
      missingSources: [
        { source: 'engine', reason: 'daemon_unavailable' },
        { source: 'quick-panel', reason: 'not_collected' },
      ],
      networkOutcome: 'unknown',
      networkReasons: ['engineOffline'],
    })

    await clickExport()

    const notice = await screen.findByText(new RegExp(`^${EXPORT}\\.missingSources\\|`))
    expect(notice.textContent).toContain(`${EXPORT}.sources.engine`)
    expect(notice.textContent).toContain(`${EXPORT}.sources.quickPanel`)
  })

  it('shows no missing-sources notice when nothing is missing', async () => {
    vi.mocked(exportLogs).mockRejectedValue(new Error('daemon unavailable'))
    vi.mocked(commands.exportStartupLogs).mockResolvedValue({
      path: '/dl/offline.zip',
      missingSources: [],
      networkOutcome: 'unknown',
      networkReasons: ['engineOffline'],
    })

    await clickExport()

    await screen.findByText('/dl/offline.zip')
    expect(screen.queryByText(new RegExp(`${EXPORT}\\.missingSources`))).not.toBeInTheDocument()
  })

  it('surfaces the daemon failure cause', async () => {
    vi.mocked(exportLogs).mockRejectedValue(daemonExportFailure('destination_unavailable'))
    vi.mocked(commands.exportStartupLogs).mockResolvedValue({
      path: '/dl/offline.zip',
      missingSources: [],
      networkOutcome: 'unknown',
      networkReasons: ['engineOffline'],
    })

    await clickExport()

    await waitFor(() =>
      expect(screen.getByText(`${EXPORT}.cause.destinationUnavailable`)).toBeInTheDocument()
    )
  })
})
