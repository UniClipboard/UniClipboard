import { fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { openUpdaterWindow, checkForUpdate } from '@/api/updater'
import { AppStatusScreen } from '@/components/app/AppStatusScreen'
import i18n from '@/i18n'

const native = vi.hoisted(() => ({ exportStartupLogs: vi.fn(), openUrl: vi.fn() }))
vi.mock('@/lib/ipc', () => ({ commands: native }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: native.openUrl }))
vi.mock('@/api/updater', () => ({ checkForUpdate: vi.fn(), openUpdaterWindow: vi.fn() }))

beforeEach(async () => {
  vi.resetAllMocks()
  await i18n.changeLanguage('zh-CN')
})
afterEach(cleanup)

describe('startup recovery screen', () => {
  it('checks for updates on ordinary failures and restores retry after a failed check', async () => {
    vi.mocked(checkForUpdate).mockRejectedValue(new Error('offline'))
    const retry = vi.fn()
    render(<AppStatusScreen detail="connection refused" onRetry={retry} />)
    fireEvent.click(screen.getByRole('button', { name: '检查更新' }))
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('无法完成更新检查'))
    expect(openUpdaterWindow).toHaveBeenCalledOnce()
    expect(checkForUpdate).toHaveBeenCalledWith(null)
    expect(screen.getByRole('button', { name: '检查更新' })).toBeEnabled()
    fireEvent.click(screen.getByRole('button', { name: '重试' }))
    expect(retry).toHaveBeenCalledOnce()
  })

  it('keeps support available during a restart', () => {
    render(<AppStatusScreen detail={null} onRetry={vi.fn()} retrying />)
    expect(screen.getByRole('button', { name: '正在重试…' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '导出日志' })).toBeEnabled()
    expect(screen.getByRole('button', { name: '联系作者' })).toBeEnabled()
  })

  it('shows a usable address when opening support fails', async () => {
    native.openUrl.mockRejectedValue(new Error('no browser'))
    render(<AppStatusScreen detail={null} onRetry={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: '联系作者' }))
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent(
        'https://github.com/UniClipboard/UniClipboard/issues/new/choose'
      )
    )
  })

  it('preserves the updater action for version mismatch failures', async () => {
    vi.mocked(openUpdaterWindow).mockResolvedValue(undefined)
    vi.mocked(checkForUpdate).mockRejectedValue(new Error('offline'))
    render(
      <AppStatusScreen
        detail="version mismatch"
        failure={{
          kind: 'versionTooOld',
          detail: 'version mismatch',
          observedVersion: '2.0.0',
          expectedVersion: '1.0.0',
        }}
        onRetry={vi.fn()}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: '打开更新' }))
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('无法完成更新检查'))
    expect(openUpdaterWindow).toHaveBeenCalledOnce()
    expect(screen.getByRole('button', { name: '导出日志' })).toBeEnabled()
  })
  it('exports logs through the native shell and shows the saved location', async () => {
    native.exportStartupLogs.mockResolvedValue('/downloads/support.zip')
    render(<AppStatusScreen detail="connection refused" onRetry={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: '导出日志' }))
    await waitFor(() =>
      expect(screen.getByRole('status')).toHaveTextContent('/downloads/support.zip')
    )
    expect(native.exportStartupLogs).toHaveBeenCalledOnce()
  })

  it('reports export failures and allows another attempt', async () => {
    native.exportStartupLogs.mockRejectedValue(new Error('disk full'))
    render(<AppStatusScreen detail={null} onRetry={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: '导出日志' }))
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('日志导出失败'))
    expect(screen.getByRole('button', { name: '导出日志' })).toBeEnabled()
  })

  it('treats cancellation as neutral and keeps retry available', async () => {
    native.exportStartupLogs.mockResolvedValue(null)
    const retry = vi.fn()
    render(<AppStatusScreen detail={null} onRetry={retry} />)
    fireEvent.click(screen.getByRole('button', { name: '导出日志' }))
    await waitFor(() => expect(screen.getByRole('button', { name: '导出日志' })).toBeEnabled())
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '重试' }))
    expect(retry).toHaveBeenCalledOnce()
  })

  it('opens the author support channel without the daemon', async () => {
    native.openUrl.mockResolvedValue(undefined)
    render(<AppStatusScreen detail={null} onRetry={vi.fn()} />)
    fireEvent.click(screen.getByRole('button', { name: '联系作者' }))
    await waitFor(() =>
      expect(native.openUrl).toHaveBeenCalledWith(
        'https://github.com/UniClipboard/UniClipboard/issues/new/choose'
      )
    )
  })
})
