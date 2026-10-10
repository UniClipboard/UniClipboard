import '@testing-library/jest-dom/vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ConfigBackupGroup } from '@/components/setting/ConfigBackupGroup'
import { commands } from '@/lib/ipc'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/api/storage', () => ({
  revealPath: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/lib/ipc', () => ({
  commands: {
    exportConfigPackage: vi.fn(),
    pickConfigBundlePath: vi.fn(),
    previewConfigImport: vi.fn(),
    importConfigPackage: vi.fn(),
    restartDaemon: vi.fn().mockResolvedValue(undefined),
    restartApp: vi.fn().mockResolvedValue(undefined),
  },
}))

vi.mock('@/components/ui/toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    message: vi.fn(),
  },
}))

const I18N = 'settings.sections.storage.configBackup'

const mockExport = vi.mocked(commands.exportConfigPackage)
const mockPick = vi.mocked(commands.pickConfigBundlePath)
const mockPreview = vi.mocked(commands.previewConfigImport)
const mockImport = vi.mocked(commands.importConfigPackage)
const mockRestartDaemon = vi.mocked(commands.restartDaemon)
const mockRestartApp = vi.mocked(commands.restartApp)

beforeEach(() => {
  vi.clearAllMocks()
})

describe('ConfigBackupGroup export', () => {
  it('exports the config in one click (no password) and reveals the bundle', async () => {
    const { revealPath } = await import('@/api/storage')
    const user = userEvent.setup()
    mockExport.mockResolvedValue({ path: '/home/test/uniclipboard-config.ucbundle' })

    render(<ConfigBackupGroup />)

    // No export password dialog: the button goes straight to the save dialog
    // (popped inside the command) and exports with the installation's own key.
    await user.click(screen.getByRole('button', { name: `${I18N}.export.button` }))

    await waitFor(() => {
      expect(mockExport).toHaveBeenCalledTimes(1)
    })
    expect(mockExport).toHaveBeenCalledWith()
    expect(vi.mocked(revealPath)).toHaveBeenCalledWith('/home/test/uniclipboard-config.ucbundle')
  })
})

describe('ConfigBackupGroup import', () => {
  it('runs pick → password → device-move confirmation → stage → restart', async () => {
    const user = userEvent.setup()
    mockPick.mockResolvedValue('/home/test/source.ucbundle')
    mockPreview.mockResolvedValue({
      appVersion: '0.16.0',
      sourceMode: 'portable',
      createdAtUnixMs: 1_700_000_000_000,
      profileId: 'default',
      deviceFingerprint: 'AB:CD:EF',
    })
    mockImport.mockResolvedValue({ stagedOk: true, unlockRequiredAfterApply: true })

    render(<ConfigBackupGroup />)

    await user.click(screen.getByRole('button', { name: `${I18N}.import.button` }))
    await waitFor(() => expect(mockPick).toHaveBeenCalledTimes(1))

    await user.type(screen.getByLabelText(`${I18N}.import.passwordLabel`), 'hunter2')
    await user.click(screen.getByRole('button', { name: `${I18N}.import.passwordConfirmButton` }))

    await waitFor(() => {
      expect(mockPreview).toHaveBeenCalledWith('hunter2', '/home/test/source.ucbundle')
    })

    // The device-move warnings must be visible before the user can confirm.
    expect(screen.getByText(`${I18N}.import.warningMove`)).toBeInTheDocument()
    expect(screen.getByText(`${I18N}.import.warningNoDualOnline`)).toBeInTheDocument()
    expect(screen.getByText(`${I18N}.import.warningReplace`)).toBeInTheDocument()
    expect(screen.getByText('0.16.0')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: `${I18N}.import.confirmButton` }))

    await waitFor(() => {
      expect(mockImport).toHaveBeenCalledWith('hunter2', '/home/test/source.ucbundle')
    })
    await waitFor(() => expect(mockRestartDaemon).toHaveBeenCalledTimes(1))
    expect(mockRestartApp).toHaveBeenCalledTimes(1)
  })

  it('surfaces a daemon error on confirm and stays on the confirm step', async () => {
    const { toast } = await import('@/components/ui/toast')
    const user = userEvent.setup()
    mockPick.mockResolvedValue('/home/test/source.ucbundle')
    mockPreview.mockResolvedValue({
      appVersion: '0.16.0',
      sourceMode: 'installed',
      createdAtUnixMs: 1_700_000_000_000,
      profileId: 'default',
      deviceFingerprint: 'AB:CD:EF',
    })
    mockImport.mockRejectedValue({
      kind: 'daemon',
      status: 400,
      code: 'INVALID_PASSWORD_OR_CORRUPT',
      message: 'invalid password or corrupt bundle',
    })

    render(<ConfigBackupGroup />)

    await user.click(screen.getByRole('button', { name: `${I18N}.import.button` }))
    await user.type(screen.getByLabelText(`${I18N}.import.passwordLabel`), 'hunter2')
    await user.click(screen.getByRole('button', { name: `${I18N}.import.passwordConfirmButton` }))
    await waitFor(() => expect(mockPreview).toHaveBeenCalled())

    await user.click(screen.getByRole('button', { name: `${I18N}.import.confirmButton` }))

    await waitFor(() => {
      expect(vi.mocked(toast).error).toHaveBeenCalledWith(`${I18N}.import.invalidPasswordError`)
    })
    expect(mockRestartDaemon).not.toHaveBeenCalled()
    // Still on the confirm step (warning copy remains visible).
    expect(screen.getByText(`${I18N}.import.warningNoDualOnline`)).toBeInTheDocument()
  })
})
