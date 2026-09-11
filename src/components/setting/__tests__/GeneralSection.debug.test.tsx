import '@testing-library/jest-dom/vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  exportLogs,
  getDiagnosticCaptureStatus,
  startDiagnosticCapture,
  stopDiagnosticCapture,
  updateDebugMode,
  type DiagnosticCaptureStatus,
} from '@/api/daemon/diagnostics'
import GeneralSection from '@/components/setting/GeneralSection'
import { toast } from '@/components/ui/toast'
import { useSetting } from '@/hooks/useSetting'
import { commands } from '@/lib/ipc'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type { SettingContextType, Settings } from '@/types/setting'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/api/daemon/diagnostics', () => ({
  updateDebugMode: vi.fn(),
  exportLogs: vi.fn(),
  getDiagnosticCaptureStatus: vi.fn(),
  startDiagnosticCapture: vi.fn(),
  stopDiagnosticCapture: vi.fn(),
}))

vi.mock('@/api/storage', () => ({
  openLogsDirectory: vi.fn(),
  revealPath: vi.fn(),
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: vi.fn(),
}))

vi.mock('@/lib/ipc', () => ({
  commands: {
    restartDaemon: vi.fn().mockResolvedValue(undefined),
    restartApp: vi.fn().mockResolvedValue(undefined),
    exportStartupLogs: vi.fn().mockResolvedValue(null),
  },
}))

vi.mock('@/components/ui/toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    message: vi.fn(),
  },
}))

const mockUseSetting = vi.mocked(useSetting)
const mockUpdateDebugMode = vi.mocked(updateDebugMode)
const mockExportLogs = vi.mocked(exportLogs)
const mockGetDiagnosticCaptureStatus = vi.mocked(getDiagnosticCaptureStatus)
const mockStartDiagnosticCapture = vi.mocked(startDiagnosticCapture)
const mockStopDiagnosticCapture = vi.mocked(stopDiagnosticCapture)
const mockRestartDaemon = vi.mocked(commands.restartDaemon)
const mockRestartApp = vi.mocked(commands.restartApp)
const mockExportStartupLogs = vi.mocked(commands.exportStartupLogs)
const mockToastMessage = vi.mocked(toast.message)

const baseSetting: Settings = makeBaseSettings()

function setup(setting: Settings = baseSetting) {
  const reloadSetting = vi.fn<SettingContextType['reloadSetting']>().mockResolvedValue(undefined)
  mockUseSetting.mockReturnValue({
    setting,
    loading: false,
    error: null,
    reloadSetting,
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
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
  return { reloadSetting }
}

beforeEach(() => {
  vi.clearAllMocks()
  mockGetDiagnosticCaptureStatus.mockResolvedValue(captureStatus())
})

function captureStatus(
  mode: DiagnosticCaptureStatus['capture']['mode'] = 'standard'
): DiagnosticCaptureStatus {
  return {
    runId: 'run-1',
    capture: {
      mode,
      captureId: mode === 'detailed' ? 'capture-1' : null,
      remainingMs: mode === 'detailed' ? 600_000 : 0,
      startedAtUtc: null,
      endReason: null,
      lastCaptureId: null,
      revision: '1',
    },
    observedRecords: '0',
    policyFilteredRecords: '0',
    schemaRejectedRecords: '0',
    correlationLimitedRecords: '0',
    engineVersion: '1.1.0',
    sourceCommit: 'abc123',
    counterScope: 'typed_events_only',
    sources: [],
    localFile: 'ready',
    closed: false,
  }
}

describe('GeneralSection debug diagnostics controls', () => {
  it('enables debug mode then restarts the daemon and app after confirmation', async () => {
    const user = userEvent.setup()
    mockUpdateDebugMode.mockResolvedValue({ debugMode: true, restartRequired: true })
    const { reloadSetting } = setup()

    render(<GeneralSection />)

    await user.click(screen.getByRole('switch', { name: /logs\.debug\.label/ }))
    expect(mockUpdateDebugMode).not.toHaveBeenCalled()
    expect(mockRestartDaemon).not.toHaveBeenCalled()
    expect(mockRestartApp).not.toHaveBeenCalled()

    await user.click(screen.getByRole('button', { name: /logs\.debug\.confirm$/ }))

    await waitFor(() => {
      expect(mockUpdateDebugMode).toHaveBeenCalledWith(true)
    })
    expect(reloadSetting).toHaveBeenCalledTimes(1)
    await waitFor(() => {
      expect(mockRestartDaemon).toHaveBeenCalledTimes(1)
    })
    expect(mockRestartApp).toHaveBeenCalledTimes(1)
  })

  it('keeps the dialog open in a forced restarting state after confirmation', async () => {
    const user = userEvent.setup()
    mockUpdateDebugMode.mockResolvedValue({ debugMode: true, restartRequired: true })
    setup()

    render(<GeneralSection />)

    await user.click(screen.getByRole('switch', { name: /logs\.debug\.label/ }))
    await user.click(screen.getByRole('button', { name: /logs\.debug\.confirm$/ }))

    // Once restarting, the dialog switches to the waiting copy and removes the
    // confirm/cancel actions so the user cannot dismiss or re-trigger it.
    await waitFor(() => {
      expect(
        screen.getByText('settings.sections.general.logs.debug.restartingDescription')
      ).toBeInTheDocument()
    })
    expect(screen.queryByRole('button', { name: /logs\.debug\.confirm$/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /logs\.debug\.cancel$/ })).not.toBeInTheDocument()

    // Escape must not close the forced dialog.
    await user.keyboard('{Escape}')
    expect(
      screen.getByText('settings.sections.general.logs.debug.restartingDescription')
    ).toBeInTheDocument()
  })

  it('exports the last 24 hours of logs and shows the path', async () => {
    const user = userEvent.setup()
    mockExportLogs.mockResolvedValue({
      path: '/home/test/Downloads/uniclipboard-debug-logs.zip',
      includedFiles: ['uniclipboard-daemon.json.2026-06-16'],
      since: '2026-06-15T00:00:00Z',
      enginePreparation: {
        flush: 'completed',
        status: captureStatus(),
        requestedAtUtc: '2026-06-16T00:00:00Z',
        completedAtUtc: '2026-06-16T00:00:01Z',
        otherProcessesFlushed: false,
        files: [],
      },
      collection: {
        includedFiles: ['uniclipboard-daemon.json.2026-06-16'],
        unreadableFiles: [],
        truncatedFiles: [],
        concurrentWritesPossible: true,
      },
    })
    setup()

    render(<GeneralSection />)

    await user.click(
      screen.getByRole('button', {
        name: 'settings.sections.general.logs.export.button',
      })
    )

    await waitFor(() => {
      expect(mockExportLogs).toHaveBeenCalledWith(24)
    })
    expect(screen.getByText('/home/test/Downloads/uniclipboard-debug-logs.zip')).toBeInTheDocument()
  })

  it('starts one daemon-owned detailed capture from the current status', async () => {
    const user = userEvent.setup()
    mockStartDiagnosticCapture.mockResolvedValue(captureStatus('detailed'))
    setup()

    render(<GeneralSection />)

    const toggle = await screen.findByRole('switch', { name: /logs\.capture\.label/ })
    await waitFor(() => expect(toggle).toBeEnabled())
    await user.click(toggle)

    await waitFor(() => expect(mockStartDiagnosticCapture).toHaveBeenCalledWith(600))
    expect(toggle).toBeChecked()
  })

  it('queries daemon state after a lost start response instead of extending capture', async () => {
    const user = userEvent.setup()
    mockStartDiagnosticCapture.mockRejectedValue(new Error('response lost'))
    mockGetDiagnosticCaptureStatus
      .mockResolvedValueOnce(captureStatus())
      .mockResolvedValueOnce(captureStatus('detailed'))
    setup()

    render(<GeneralSection />)

    const toggle = await screen.findByRole('switch', { name: /logs\.capture\.label/ })
    await waitFor(() => expect(toggle).toBeEnabled())
    await user.click(toggle)

    await waitFor(() => expect(toggle).toBeChecked())
    expect(mockStartDiagnosticCapture).toHaveBeenCalledTimes(1)
    expect(mockGetDiagnosticCaptureStatus).toHaveBeenCalledTimes(2)
  })

  it('stops only the capture identifier reported by the daemon', async () => {
    const user = userEvent.setup()
    mockGetDiagnosticCaptureStatus
      .mockResolvedValueOnce(captureStatus('detailed'))
      .mockResolvedValueOnce(captureStatus())
    mockStopDiagnosticCapture.mockResolvedValue('stopped')
    setup()

    render(<GeneralSection />)

    const toggle = await screen.findByRole('switch', { name: /logs\.capture\.label/ })
    await waitFor(() => expect(toggle).toBeChecked())
    await user.click(toggle)

    await waitFor(() => expect(mockStopDiagnosticCapture).toHaveBeenCalledWith('capture-1'))
    await waitFor(() => expect(toggle).not.toBeChecked())
  })

  it('falls back to retained logs when the daemon export is unavailable', async () => {
    const user = userEvent.setup()
    mockExportLogs.mockRejectedValue(new Error('daemon unavailable'))
    mockExportStartupLogs.mockResolvedValue('/home/test/Downloads/offline-diagnostics.zip')
    setup()

    render(<GeneralSection />)
    await user.click(
      screen.getByRole('button', { name: 'settings.sections.general.logs.export.button' })
    )

    await waitFor(() => expect(mockExportStartupLogs).toHaveBeenCalledOnce())
    expect(screen.getByText('/home/test/Downloads/offline-diagnostics.zip')).toBeInTheDocument()
  })

  it('reports incomplete live export coverage without calling it complete', async () => {
    const user = userEvent.setup()
    mockExportLogs.mockResolvedValue({
      path: '/home/test/Downloads/partial-diagnostics.zip',
      includedFiles: ['engine.2026-09-11.jsonl'],
      since: '2026-09-11T00:00:00Z',
      enginePreparation: {
        flush: 'timedOut',
        status: captureStatus(),
        requestedAtUtc: '2026-09-11T00:00:00Z',
        completedAtUtc: '2026-09-11T00:00:01Z',
        otherProcessesFlushed: false,
        files: [],
      },
      collection: {
        includedFiles: ['engine.2026-09-11.jsonl'],
        unreadableFiles: [],
        truncatedFiles: [],
        concurrentWritesPossible: true,
      },
    })
    setup()

    render(<GeneralSection />)
    await user.click(
      screen.getByRole('button', { name: 'settings.sections.general.logs.export.button' })
    )

    await waitFor(() =>
      expect(mockToastMessage).toHaveBeenCalledWith(
        'settings.sections.general.logs.export.partialSuccess'
      )
    )
  })
})
