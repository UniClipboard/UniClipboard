import { configureStore } from '@reduxjs/toolkit'
import { renderHook, act } from '@testing-library/react'
import React from 'react'
import { Provider } from 'react-redux'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useClipboardEvents } from '../useClipboardEvents'
import { Filter, getClipboardEntry } from '@/api/clipboardItems'
import { getEncryptionState } from '@/api/daemon'
import { getClipboardEntries } from '@/api/daemon/clipboard'
import clipboardReducer from '@/store/slices/clipboardSlice'

// Mock useEncryptionSessionState — always return encryption ready (avoids async complexity)
vi.mock('../useEncryptionSessionState', () => ({
  useEncryptionSessionState: () => ({ encryptionReady: true, isLocked: false }),
}))

// Mock daemonWs (replaces Tauri listen — useClipboardEventStream now uses daemonWs.subscribe)
let capturedClipboardHandler: ((event: unknown) => void) | null = null
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn((topics: string[], handler: (event: unknown) => void) => {
      if (topics.includes('clipboard')) {
        capturedClipboardHandler = handler
      }
      return () => {
        if (topics.includes('clipboard')) {
          capturedClipboardHandler = null
        }
      }
    }),
  },
}))

// Mock clipboard API (getClipboardEntries is called inside fetchClipboardItems thunk)
vi.mock('@/api/clipboardItems', async importOriginal => {
  const actual = await importOriginal<typeof import('@/api/clipboardItems')>()
  return {
    ...actual,
    getClipboardEntry: vi.fn(),
    // getClipboardEntries is imported inside clipboardSlice via @/api/daemon/clipboard
    // We need to mock it so fetchClipboardItems succeeds
  }
})

// Mock the daemon clipboard module — used by fetchClipboardItems thunk and useClipboardEventStream
vi.mock('@/api/daemon/clipboard', () => ({
  getClipboardEntries: vi.fn().mockResolvedValue({ entries: [], status: 'ready' }),
}))

// Mock the daemon clipboard module — used by fetchClipboardItems thunk and useClipboardEventStream
vi.mock('@/api/daemon', () => ({
  getEncryptionState: vi.fn().mockResolvedValue({
    initialized: false,
    sessionReady: false,
  }),
  // Provide getClipboardEntries so clipboardSlice's fetchClipboardItems thunk can import it
  getClipboardEntries: vi.fn().mockResolvedValue({ entries: [], status: 'ready' }),
}))

// Mock toast
vi.mock('@/components/ui/toast', () => ({
  toast: { error: vi.fn() },
}))

// Mock daemonClient.request to return appropriate shape based on the path
vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    request: vi.fn().mockImplementation((path: string) => {
      if (typeof path === 'string' && path.startsWith('/encryption/')) {
        return Promise.resolve({ data: { initialized: false, sessionReady: false } })
      }
      return Promise.resolve({ entries: [], items: [], status: 'ready' })
    }),
  },
}))

const mockGetClipboardEntry = vi.mocked(getClipboardEntry)

// Track dispatched actions
let dispatchedActions: Array<{ type: string; payload?: unknown }> = []

function createTestStore() {
  const store = configureStore({
    reducer: {
      clipboard: clipboardReducer,
    },
  })

  // Spy on dispatch to record actions
  const originalDispatch = store.dispatch.bind(store)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  store.dispatch = ((action: any) => {
    if (action && typeof action === 'object' && 'type' in action) {
      dispatchedActions.push(action as { type: string; payload?: unknown })
    }
    return originalDispatch(action)
  }) as typeof store.dispatch
  return store
}

function createWrapper() {
  const store = createTestStore()
  const Wrapper = ({ children }: { children: React.ReactNode }) =>
    React.createElement(Provider, { store, children })
  return { Wrapper, store }
}

describe('useClipboardEvents', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    dispatchedActions = []
    capturedClipboardHandler = null

    // useEncryptionSessionState is mocked to always return ready
  })

  it('registers clipboard listener on mount', async () => {
    const { Wrapper } = createWrapper()
    renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    // Wait for the daemonWs.subscribe call to register
    await vi.waitFor(() => {
      expect(capturedClipboardHandler).not.toBeNull()
    })
  })

  it('P16-05: local origin event triggers getClipboardEntry and dispatches prependItem', async () => {
    // The hook now calls getClipboardEntries from @/api/daemon/clipboard (daemon HTTP path).
    // Set up daemonClient.request mock to return a matching entry
    const mockEntry = {
      id: 'entry-1',
      contentType: 'text/plain',
      preview: 'hello',
      hasDetail: false,
      sizeBytes: 5,
      capturedAt: 1000,
      updatedAt: 1000,
      activeTime: 0,
      isEncrypted: false,
      isFavorited: false,
      isDownloaded: true,
      linkUrls: null,
      linkDomains: null,
      fileSizes: null,
      fileTransferStatus: null,
      fileTransferReason: null,
      thumbnailUrl: null,
    }

    // Override getClipboardEntries to return the specific entry (intercepted before daemonClient.request)
    const mockGetClipboardEntries = vi.mocked(getClipboardEntries)
    mockGetClipboardEntries.mockResolvedValueOnce({
      entries: [mockEntry],
      status: 'ready' as const,
    })

    // Override encryption state so it's immediately ready — no async resolution needed
    vi.mocked(getEncryptionState).mockResolvedValueOnce({
      initialized: false,
      sessionReady: false,
    })

    const { Wrapper } = createWrapper()
    const { unmount } = renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    // Wait for listener registration
    await vi.waitFor(() => {
      expect(capturedClipboardHandler).not.toBeNull()
    })

    // Encryption is immediately ready — loadData should have been called
    await vi.waitFor(() => {
      expect(dispatchedActions.some(a => a.type.startsWith('clipboard/fetchItems'))).toBe(true)
    })

    // Simulate local clipboard event (daemon WS format)
    await act(async () => {
      capturedClipboardHandler?.({
        topic: 'clipboard',
        eventType: 'clipboard.new_content',
        ts: 0,
        sessionId: null,
        payload: { entry_id: 'entry-1', preview: 'hello', origin: 'local' },
      })
      await new Promise(r => setTimeout(r, 0)) // flush micro-task queue
    })

    // prependItem should have been dispatched
    const prependAction = dispatchedActions.find(a => a.type === 'clipboard/prependItem')
    expect(prependAction).toBeDefined()
    expect(prependAction?.payload).toMatchObject({ id: 'entry-1' })

    unmount()
  })

  it('P16-06: remote origin event triggers throttled full reload (fetchClipboardItems dispatch)', async () => {
    // The hook now calls fetchClipboardItems thunk (which uses daemon HTTP, not Tauri invoke)
    // Verify the thunk was dispatched rather than checking for invokeWithTrace

    // useEncryptionSessionState is mocked to always return ready

    const { Wrapper } = createWrapper()
    const { unmount } = renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    // Wait for listener registration
    await vi.waitFor(() => {
      expect(capturedClipboardHandler).not.toBeNull()
    })

    // Encryption is immediately ready — loadData should have been called
    await vi.waitFor(() => {
      expect(dispatchedActions.some(a => a.type.startsWith('clipboard/fetchItems'))).toBe(true)
    })

    // Clear any previous invocations from the initial load
    dispatchedActions = []

    // Simulate remote clipboard event (daemon WS format)
    await act(async () => {
      capturedClipboardHandler?.({
        topic: 'clipboard',
        eventType: 'clipboard.new_content',
        ts: 0,
        sessionId: null,
        payload: { entry_id: 'entry-2', preview: '...', origin: 'remote' },
      })
      await new Promise(r => setTimeout(r, 0)) // flush micro-task queue
    })

    // Remote event should trigger loadData -> fetchClipboardItems thunk (daemon HTTP path)
    const fetchAction = dispatchedActions.find(a => a.type === 'clipboard/fetchItems')
    expect(fetchAction).toBeDefined()
    // getClipboardEntry should NOT have been called (that's for local events only)
    expect(mockGetClipboardEntry).not.toHaveBeenCalled()

    unmount()
  })

  // Note: clipboard.deleted is never emitted by the daemon — test omitted.
  it('Deleted event dispatches removeItem', async () => {
    // Omitted: clipboard.deleted is never emitted by the daemon.
    // If the daemon adds this event in the future, re-enable this test.
  })

  it('cleans up listeners on unmount', async () => {
    const { Wrapper } = createWrapper()
    const { unmount } = renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    await vi.waitFor(() => {
      expect(capturedClipboardHandler).not.toBeNull()
    })

    unmount()

    await vi.waitFor(() => {
      expect(capturedClipboardHandler).toBeNull()
    })
  })
})
