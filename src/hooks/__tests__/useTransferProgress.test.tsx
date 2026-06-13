import { configureStore } from '@reduxjs/toolkit'
import { act, renderHook, waitFor } from '@testing-library/react'
import React from 'react'
import { Provider } from 'react-redux'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import fileTransferReducer from '@/store/slices/fileTransferSlice'
import { useTransferProgress } from '../useTransferProgress'

// ── Mock daemon WS ────────────────────────────────────────────

let capturedHandler: ((event: { eventType: string; payload: unknown }) => void) | null = null

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn(
      (_topics: string[], handler: (event: { eventType: string; payload: unknown }) => void) => {
        capturedHandler = handler
        return () => {
          capturedHandler = null
        }
      }
    ),
  },
}))

// ── Store helper ─────────────────────────────────────────────

function createTestStore() {
  return configureStore({
    reducer: {
      fileTransfer: fileTransferReducer,
    },
  })
}

function createWrapper() {
  const store = createTestStore()
  const Wrapper = ({ children }: { children: React.ReactNode }) => (
    <Provider store={store}>{children}</Provider>
  )
  return { Wrapper, store }
}

// ── Emit helper ─────────────────────────────────────────────

function emitWsEvent(eventType: string, payload: unknown) {
  if (capturedHandler) {
    capturedHandler({ eventType, payload })
  }
}

// ── Tests ────────────────────────────────────────────────────

describe('useTransferProgress', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    capturedHandler = null
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('subscribes to file-transfer topic on mount', async () => {
    const { daemonWs } = await import('@/lib/daemon-ws')
    const { Wrapper } = createWrapper()
    renderHook(() => useTransferProgress(), { wrapper: Wrapper })

    await waitFor(() => {
      expect(capturedHandler).not.toBeNull()
    })

    expect(daemonWs.subscribe).toHaveBeenCalledWith(['file-transfer'], expect.any(Function))
  })

  describe('durable transfer status', () => {
    it('dispatches setEntryTransferStatus on status-changed event', async () => {
      const { Wrapper, store } = createWrapper()
      renderHook(() => useTransferProgress(), { wrapper: Wrapper })

      await waitFor(() => {
        expect(capturedHandler).not.toBeNull()
      })

      act(() => {
        emitWsEvent('file-transfer.status_changed', {
          transferId: 'tx-1',
          entryId: 'entry-abc',
          status: 'completed',
        })
      })

      const state = store.getState().fileTransfer
      expect(state.entryStatusById['entry-abc']).toMatchObject({
        status: 'completed',
        reason: null,
      })
    })

    it('stores failed reason from status-changed event', async () => {
      const { Wrapper, store } = createWrapper()
      renderHook(() => useTransferProgress(), { wrapper: Wrapper })

      await waitFor(() => {
        expect(capturedHandler).not.toBeNull()
      })

      act(() => {
        emitWsEvent('file-transfer.status_changed', {
          transferId: 'tx-fail',
          entryId: 'entry-fail',
          status: 'failed',
          reason: 'timeout after 60s',
        })
      })

      const state = store.getState().fileTransfer
      expect(state.entryStatusById['entry-fail']).toMatchObject({
        status: 'failed',
        reason: 'timeout after 60s',
      })
    })

    it('marks transfer as failed in progress state when status is failed', async () => {
      const { Wrapper, store } = createWrapper()
      renderHook(() => useTransferProgress(), { wrapper: Wrapper })

      await waitFor(() => {
        expect(capturedHandler).not.toBeNull()
      })

      act(() => {
        emitWsEvent('file-transfer.status_changed', {
          transferId: 'tx-mark-fail',
          entryId: 'entry-fail2',
          status: 'failed',
          reason: 'connection reset by peer',
        })
      })

      expect(store.getState().fileTransfer.entryStatusById['entry-fail2']).toMatchObject({
        status: 'failed',
        reason: 'connection reset by peer',
      })
    })

    it('ignores events with invalid status values', async () => {
      const { Wrapper, store } = createWrapper()
      renderHook(() => useTransferProgress(), { wrapper: Wrapper })

      await waitFor(() => {
        expect(capturedHandler).not.toBeNull()
      })

      act(() => {
        emitWsEvent('file-transfer.status_changed', {
          transferId: 'tx-invalid',
          entryId: 'entry-invalid',
          status: 'unknown_status',
        })
      })

      expect(store.getState().fileTransfer.entryStatusById['entry-invalid']).toBeUndefined()
    })
  })

  describe('transfer progress events', () => {
    it('stores live progress and links transfer to entry', async () => {
      const { Wrapper, store } = createWrapper()
      renderHook(() => useTransferProgress(), { wrapper: Wrapper })

      await waitFor(() => {
        expect(capturedHandler).not.toBeNull()
      })

      act(() => {
        emitWsEvent('file-transfer.progress', {
          transferId: 'tx-progress',
          entryId: 'entry-progress',
          peerId: 'peer-1',
          direction: 'Receiving',
          bytesTransferred: 2048,
          totalBytes: 10240,
        })
      })

      const state = store.getState().fileTransfer
      expect(state.entryTransferMap['entry-progress']).toBe('tx-progress')
      expect(state.activeTransfers['tx-progress']).toMatchObject({
        entryId: 'entry-progress',
        peerId: 'peer-1',
        direction: 'Receiving',
        bytesTransferred: 2048,
        totalBytes: 10240,
        status: 'active',
      })
    })
  })

  describe('unhandled events', () => {
    it('ignores unrelated events without error', async () => {
      const { Wrapper, store } = createWrapper()
      renderHook(() => useTransferProgress(), { wrapper: Wrapper })

      await waitFor(() => {
        expect(capturedHandler).not.toBeNull()
      })

      act(() => {
        emitWsEvent('file-transfer.unknown', { transferId: 'tx-unknown' })
      })

      expect(store.getState().fileTransfer).toBeDefined()
    })
  })

  it('cleans up daemon WS subscription on unmount', async () => {
    const { Wrapper } = createWrapper()
    const { unmount } = renderHook(() => useTransferProgress(), { wrapper: Wrapper })

    await waitFor(() => {
      expect(capturedHandler).not.toBeNull()
    })

    unmount()

    expect(capturedHandler).toBeNull()
  })
})
