import { act, renderHook, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { useAppBootstrap } from '@/hooks/useAppBootstrap'
import { refreshStartupSnapshot } from '@/lib/daemon-startup-progress'

const mocks = vi.hoisted(() => ({
  restart: vi.fn().mockResolvedValue(undefined),
  connect: vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue(undefined),
  refresh: vi.fn().mockResolvedValue(undefined),
  startup: vi.fn().mockResolvedValue(null),
  check: vi.fn(() => ({
    unwrap: () => Promise.resolve({ initialized: true, session_ready: false }),
  })),
  refetch: vi.fn(() => {
    throw new Error('Cannot refetch a query that has not been started yet')
  }),
}))
vi.mock('@/lib/ipc', () => ({
  commands: {
    restartDaemon: mocks.restart,
    getDaemonBootstrapFailure: vi.fn().mockResolvedValue(null),
    getDaemonStartupStatus: mocks.startup,
  },
}))
vi.mock('@/api/daemon/client', () => ({ daemonClient: { refreshSession: mocks.refresh } }))
vi.mock('@/api/daemon/lifecycle', () => ({
  signalLifecycleReady: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('@/lib/daemon-ws-bootstrap', () => ({ connectDaemonWs: mocks.connect }))
vi.mock('@/hooks/useDaemonEvents', () => ({ useEncryptionState: vi.fn() }))
vi.mock('@/observability/errors', () => ({ reportError: vi.fn() }))
vi.mock('@/store/api', () => ({
  useGetEncryptionSessionStatusQuery: () => ({ isLoading: false, refetch: mocks.refetch }),
  useLazyGetEncryptionSessionStatusQuery: () => [mocks.check],
}))

describe('startup retry', () => {
  it('automatically reconnects when a delayed startup becomes ready', async () => {
    mocks.connect
      .mockReset()
      .mockRejectedValueOnce(new Error('old timeout'))
      .mockResolvedValue(undefined)
    mocks.startup.mockResolvedValue({
      package_version: '1.0.0',
      service_ready: false,
      service_failed: false,
      progress: { state: 'upgrading', attempt_id: 'delayed', sequence: 1 },
    })
    const { result, unmount } = renderHook(() => useAppBootstrap(false))
    await waitFor(() => expect(result.current.encryptionError).toBe('old timeout'))
    mocks.startup.mockResolvedValue({
      package_version: '1.0.0',
      service_ready: true,
      service_failed: false,
      progress: { state: 'ready', attempt_id: 'delayed', sequence: 2 },
    })
    await waitFor(() => expect(result.current.daemonBootstrapReady).toBe(true))
    expect(result.current.encryptionError).toBeNull()
    expect(mocks.restart).not.toHaveBeenCalled()
    unmount()
    mocks.startup.mockResolvedValue(null)
    await refreshStartupSnapshot()
    mocks.connect
      .mockReset()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValue(undefined)
  })
  it('starts a fresh check after an initial failure and prevents simultaneous restarts', async () => {
    const { result, unmount } = renderHook(() => useAppBootstrap(false))
    await waitFor(() => expect(result.current.encryptionError).toBe('offline'))
    act(() => {
      result.current.retry()
      result.current.retry()
    })
    await waitFor(() => expect(result.current.retrying).toBe(false))
    expect(mocks.restart).toHaveBeenCalledOnce()
    expect(mocks.check).toHaveBeenCalledWith(undefined, false)
    expect(result.current.encryptionError).toBeNull()
    expect(result.current.daemonBootstrapReady).toBe(true)
    unmount()
  })
})
