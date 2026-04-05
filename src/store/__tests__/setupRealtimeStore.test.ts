import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getSetupState, onSetupStateChanged, onSpaceAccessCompleted } from '@/api/setup'
import type { SetupStateChangedEvent, SpaceAccessCompletedEvent } from '@/api/setup'
import {
  ensureSetupRealtimeSync,
  resetSetupRealtimeStoreForTests,
  useSetupRealtimeStore,
} from '@/store/setupRealtimeStore'

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/api/setup', () => ({
  getSetupState: vi.fn(),
  onSetupStateChanged: vi.fn(),
  onSpaceAccessCompleted: vi.fn(),
  handleSpaceAccessCompleted: vi.fn(),
}))

describe('setupRealtimeStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    resetSetupRealtimeStoreForTests()
  })

  it('hydrates once and then advances from setup realtime events', async () => {
    const stopListening = vi.fn()
    let realtimeCallback:
      | ((event: {
          sessionId: string
          state: {
            JoinSpaceConfirmPeer: { short_code: string; peer_fingerprint: string; error: null }
          }
          ts: number
        }) => void)
      | null = null

    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    vi.mocked(onSetupStateChanged).mockImplementation(async callback => {
      realtimeCallback = callback
      return stopListening
    })

    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(result.current.hydrated).toBe(true)
    })

    expect(result.current.setupState).toBe('Welcome')
    expect(result.current.sessionId).toBeNull()
    expect(getSetupState).toHaveBeenCalledTimes(1)

    act(() => {
      realtimeCallback?.({
        sessionId: 'session-setup',
        state: {
          JoinSpaceConfirmPeer: {
            short_code: '123456',
            peer_fingerprint: 'peer-fp',
            error: null,
          },
        },
        ts: 1,
      })
    })

    expect(result.current.setupState).toEqual({
      JoinSpaceConfirmPeer: {
        short_code: '123456',
        peer_fingerprint: 'peer-fp',
        error: null,
      },
    })
    expect(result.current.sessionId).toBe('session-setup')
  })

  it('hydrates from the setup state returned by the API facade', async () => {
    vi.mocked(getSetupState).mockResolvedValue({
      CreateSpaceInputPassphrase: { error: null },
    })
    vi.mocked(onSetupStateChanged).mockResolvedValue(() => {})
    vi.mocked(onSpaceAccessCompleted).mockResolvedValue(() => {})

    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(result.current.hydrated).toBe(true)
    })

    expect(result.current.setupState).toEqual({
      CreateSpaceInputPassphrase: { error: null },
    })
  })

  it('applies command responses without rehydrating setup state', async () => {
    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    vi.mocked(onSetupStateChanged).mockResolvedValue(() => {})

    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(result.current.hydrated).toBe(true)
    })

    act(() => {
      result.current.syncSetupStateFromCommand({
        ProcessingJoinSpace: { message: 'waiting for pairing verification' },
      })
    })

    expect(result.current.setupState).toEqual({
      ProcessingJoinSpace: { message: 'waiting for pairing verification' },
    })
    expect(getSetupState).toHaveBeenCalledTimes(1)
  })

  it('nulls sessionId when state transitions to Completed', async () => {
    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    const stopListening = vi.fn()
    vi.mocked(onSetupStateChanged).mockImplementation(async callback => {
      // Immediately invoke callback to simulate existing session
      callback({
        sessionId: 'sess-1',
        state: {
          JoinSpaceConfirmPeer: {
            short_code: '123456',
            peer_fingerprint: 'peer-fp',
            error: null,
          },
        },
        ts: 1,
      })
      return stopListening
    })

    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(result.current.sessionId).toBe('sess-1')
    })

    act(() => {
      result.current.syncSetupStateFromCommand('Completed')
    })

    expect(result.current.setupState).toBe('Completed')
    expect(result.current.sessionId).toBeNull()
  })

  it('nulls sessionId when state transitions to Welcome', async () => {
    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    const stopListening = vi.fn()
    vi.mocked(onSetupStateChanged).mockImplementation(async callback => {
      // Immediately invoke callback to simulate existing session
      callback({
        sessionId: 'sess-2',
        state: {
          JoinSpaceConfirmPeer: {
            short_code: '654321',
            peer_fingerprint: 'peer-fp',
            error: null,
          },
        },
        ts: 1,
      })
      return stopListening
    })

    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(result.current.sessionId).toBe('sess-2')
    })

    act(() => {
      result.current.syncSetupStateFromCommand('Welcome')
    })

    expect(result.current.setupState).toBe('Welcome')
    expect(result.current.sessionId).toBeNull()
  })

  it('resetSetupRealtimeStoreForTests restores default snapshot and can re-hydrate', async () => {
    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    vi.mocked(onSetupStateChanged).mockResolvedValue(() => {})
    vi.mocked(onSpaceAccessCompleted).mockResolvedValue(() => {})

    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(result.current.hydrated).toBe(true)
    })

    act(() => {
      resetSetupRealtimeStoreForTests()
    })

    // After reset, snapshot is cleared immediately
    expect(result.current.setupState).toBeNull()
    expect(result.current.sessionId).toBeNull()

    await act(async () => {
      await ensureSetupRealtimeSync()
    })

    await waitFor(() => {
      expect(result.current.hydrated).toBe(true)
    })
    expect(result.current.setupState).toBe('Welcome')
  })

  it('cleans up the realtime listener when the singleton store resets', async () => {
    const stopListening = vi.fn()

    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    vi.mocked(onSetupStateChanged).mockResolvedValue(stopListening)

    renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(onSetupStateChanged).toHaveBeenCalledTimes(1)
    })

    act(() => {
      resetSetupRealtimeStoreForTests()
    })

    expect(stopListening).toHaveBeenCalledTimes(1)
  })

  // ── Observability path tests ────────────────────────────────────────────────────────

  it('logs skipped decision when ensureSetupRealtimeSync is called while already running', async () => {
    const consoleSpy = vi.spyOn(console, 'debug').mockImplementation(() => {})

    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    vi.mocked(onSetupStateChanged).mockResolvedValue(() => {})
    vi.mocked(onSpaceAccessCompleted).mockResolvedValue(() => {})

    renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(onSetupStateChanged).toHaveBeenCalledTimes(1)
    })

    // Call again while already running — should log 'skipped already_running'
    await act(async () => {
      await ensureSetupRealtimeSync()
    })

    const skippedLogs = consoleSpy.mock.calls
      .map(args => (args[0] as string) || '')
      .filter(
        msg => msg.includes('[setupRealtimeStore] skipped') && msg.includes('already_running')
      )
    expect(skippedLogs.length).toBeGreaterThan(0)

    consoleSpy.mockRestore()
  })

  it('logs space_access_ignored decision when setup is already Completed on the sponsor side', async () => {
    const consoleSpy = vi.spyOn(console, 'debug').mockImplementation(() => {})
    let spaceAccessCallback: ((event: SpaceAccessCompletedEvent) => void) | null = null

    vi.mocked(getSetupState).mockResolvedValue('Completed')
    vi.mocked(onSetupStateChanged).mockResolvedValue(() => {})
    vi.mocked(onSpaceAccessCompleted).mockImplementation(async callback => {
      spaceAccessCallback = callback
      return () => {}
    })

    renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(onSpaceAccessCompleted).toHaveBeenCalledTimes(1)
    })

    // Fire space access completed while setup is already 'Completed' (sponsor side behavior)
    await act(async () => {
      spaceAccessCallback?.({
        sessionId: 'sess-sponsor',
        peerId: 'peer-sponsor',
        success: true,
        ts: 1,
      })
    })

    // Observability: space_access_ignored must be logged with setup_already_completed reason
    const ignoredLogs = consoleSpy.mock.calls
      .map(args => (args[0] as string) || '')
      .filter(
        msg =>
          msg.includes('[setupRealtimeStore] space_access_ignored') &&
          msg.includes('setup_already_completed')
      )
    expect(ignoredLogs.length).toBeGreaterThan(0)

    consoleSpy.mockRestore()
  })

  it('logs started and running decisions across a successful initialization', async () => {
    const consoleSpy = vi.spyOn(console, 'debug').mockImplementation(() => {})

    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    vi.mocked(onSetupStateChanged).mockResolvedValue(() => {})
    vi.mocked(onSpaceAccessCompleted).mockResolvedValue(() => {})

    renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(onSetupStateChanged).toHaveBeenCalledTimes(1)
    })

    const allMessages = consoleSpy.mock.calls.map(args => (args[0] as string) || '')

    const startedLog = allMessages.find(msg => msg.includes('[setupRealtimeStore] started'))
    expect(startedLog).toBeTruthy()

    const runningLog = allMessages.find(msg => msg.includes('[setupRealtimeStore] running'))
    expect(runningLog).toBeTruthy()

    consoleSpy.mockRestore()
  })

  it('does not silently drop deduped state events — they get skipped only by setup.ts, not the store', async () => {
    // The store itself should apply every callback call it receives.
    // Deduplication is the responsibility of setup.ts (onSetupStateChanged), not the store.
    // This test verifies the store applies each realtime event it receives without internal dedupe.
    let realtimeCallback: ((event: SetupStateChangedEvent) => void) | null = null

    vi.mocked(getSetupState).mockResolvedValue('Welcome')
    vi.mocked(onSetupStateChanged).mockImplementation(async callback => {
      realtimeCallback = callback
      return () => {}
    })

    const { result } = renderHook(() => useSetupRealtimeStore())

    await waitFor(() => {
      expect(result.current.hydrated).toBe(true)
    })

    // Send the same state twice — store applies both because deduplication happens upstream
    const state = {
      JoinSpaceConfirmPeer: { short_code: 'abc', peer_fingerprint: 'fp', error: null },
    }

    act(() => {
      realtimeCallback?.({ sessionId: 'sess-dedup', state, ts: 100 })
    })
    expect(result.current.setupState).toEqual(state)
    expect(result.current.sessionId).toBe('sess-dedup')

    act(() => {
      realtimeCallback?.({ sessionId: 'sess-dedup', state, ts: 100 })
    })
    // Store still reflects the state — no silent null/reset
    expect(result.current.setupState).toEqual(state)
  })
})
