import { configureStore } from '@reduxjs/toolkit'
import { listen } from '@tauri-apps/api/event'
import { renderHook, act } from '@testing-library/react'
import React from 'react'
import { Provider } from 'react-redux'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useClipboardEvents } from '../useClipboardEvents'
import { Filter, getClipboardEntry } from '@/api/clipboardItems'
import { getEncryptionSessionStatus } from '@/api/security'
import { invokeWithTrace } from '@/lib/tauri-command'
import clipboardReducer from '@/store/slices/clipboardSlice'

// Mock Tauri event API
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

// Mock clipboard API
vi.mock('@/api/clipboardItems', async importOriginal => {
  const actual = await importOriginal<typeof import('@/api/clipboardItems')>()
  return {
    ...actual,
    getClipboardEntry: vi.fn(),
  }
})

// Mock security API
vi.mock('@/api/security', () => ({
  getEncryptionSessionStatus: vi.fn().mockResolvedValue({
    initialized: false,
    session_ready: false,
  }),
}))

// Mock toast
vi.mock('@/components/ui/toast', () => ({
  toast: { error: vi.fn() },
}))

// Mock tauri invoke to prevent errors when fetchClipboardItems thunk fires
vi.mock('@/lib/tauri-command', () => ({
  invokeWithTrace: vi.fn().mockResolvedValue({ status: 'ready', entries: [] }),
}))

const mockListen = vi.mocked(listen)
const mockGetClipboardEntry = vi.mocked(getClipboardEntry)
const mockGetEncryptionSessionStatus = vi.mocked(getEncryptionSessionStatus)
const mockInvokeWithTrace = vi.mocked(invokeWithTrace)

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
  let clipboardListenerCallback: ((event: { payload: unknown }) => void) | null = null
  const mockUnlisten = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
    dispatchedActions = []
    clipboardListenerCallback = null

    mockListen.mockImplementation(async (channel: string, callback: unknown) => {
      if (channel === 'clipboard://event') {
        clipboardListenerCallback = callback as (event: { payload: unknown }) => void
      }
      return mockUnlisten
    })

    // Default: encryption not initialized (so ready = true per hook logic)
    mockGetEncryptionSessionStatus.mockResolvedValue({
      initialized: false,
      session_ready: false,
    })
  })

  it('registers clipboard and encryption event listeners on mount', async () => {
    const { Wrapper } = createWrapper()
    renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    await vi.waitFor(() => {
      expect(mockListen).toHaveBeenCalledWith('clipboard://event', expect.any(Function))
      expect(mockListen).toHaveBeenCalledWith('encryption://event', expect.any(Function))
    })
  })

  it('P16-05: local origin event triggers getClipboardEntry and dispatches prependItem', async () => {
    const mockItem = {
      id: 'entry-1',
      is_downloaded: true,
      is_favorited: false,
      created_at: 1000,
      updated_at: 1000,
      active_time: 0,
      item: {
        text: { display_text: 'hello', has_detail: false, size: 5 },
        image: null,
        file: null,
        link: null,
        code: null,
        unknown: null,
      },
    }
    mockGetClipboardEntry.mockResolvedValue(mockItem)

    const { Wrapper } = createWrapper()
    renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    // Wait for listeners + encryption check
    await vi.waitFor(() => {
      expect(clipboardListenerCallback).not.toBeNull()
    })

    // Wait for encryption status check to mark ready
    await act(async () => {
      await new Promise(r => setTimeout(r, 10))
    })

    // Simulate local clipboard event
    await act(async () => {
      clipboardListenerCallback!({
        payload: { type: 'NewContent', entry_id: 'entry-1', origin: 'local' },
      })
      await new Promise(r => setTimeout(r, 10))
    })

    expect(mockGetClipboardEntry).toHaveBeenCalledWith('entry-1')

    const prependAction = dispatchedActions.find(a => a.type === 'clipboard/prependItem')
    expect(prependAction).toBeDefined()
    expect(prependAction?.payload).toEqual(mockItem)
  })

  it('P16-06: remote origin event triggers throttled full reload (fetchClipboardItems dispatch)', async () => {
    const { Wrapper } = createWrapper()
    renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    await vi.waitFor(() => {
      expect(clipboardListenerCallback).not.toBeNull()
    })

    // Wait for encryption ready
    await act(async () => {
      await new Promise(r => setTimeout(r, 10))
    })

    // Clear any previous invocations from the initial load
    mockInvokeWithTrace.mockClear()

    // Simulate remote clipboard event
    await act(async () => {
      clipboardListenerCallback!({
        payload: { type: 'NewContent', entry_id: 'entry-2', origin: 'remote' },
      })
      // Wait for async thunk to dispatch and invoke
      await new Promise(r => setTimeout(r, 30))
    })

    // Remote event should trigger loadData which calls fetchClipboardItems -> invokeWithTrace('get_clipboard_entries', ...)
    expect(mockInvokeWithTrace).toHaveBeenCalledWith(
      'get_clipboard_entries',
      expect.objectContaining({ limit: 20, offset: 0 })
    )
    // getClipboardEntry should NOT have been called (that's for local events)
    expect(mockGetClipboardEntry).not.toHaveBeenCalled()
  })

  it('Deleted event dispatches removeItem', async () => {
    const { Wrapper } = createWrapper()
    renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    await vi.waitFor(() => {
      expect(clipboardListenerCallback).not.toBeNull()
    })

    await act(async () => {
      clipboardListenerCallback!({
        payload: { type: 'Deleted', entry_id: 'entry-del' },
      })
    })

    const removeAction = dispatchedActions.find(a => a.type === 'clipboard/removeItem')
    expect(removeAction).toBeDefined()
    expect(removeAction?.payload).toBe('entry-del')
  })

  it('encryption not ready: events are ignored', async () => {
    // Encryption initialized but session NOT ready
    mockGetEncryptionSessionStatus.mockResolvedValue({
      initialized: true,
      session_ready: false,
    })

    const { Wrapper } = createWrapper()
    renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    // Wait for encryption status check to complete
    await act(async () => {
      await new Promise(r => setTimeout(r, 10))
    })
    expect(clipboardListenerCallback).toBeNull()

    expect(mockGetClipboardEntry).not.toHaveBeenCalled()
  })

  it('cleans up listeners on unmount', async () => {
    const { Wrapper } = createWrapper()
    const { unmount } = renderHook(() => useClipboardEvents(Filter.All), { wrapper: Wrapper })

    await vi.waitFor(() => {
      expect(mockListen).toHaveBeenCalled()
    })

    unmount()

    await vi.waitFor(() => {
      expect(mockUnlisten).toHaveBeenCalled()
    })
  })
})
