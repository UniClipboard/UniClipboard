import { listen } from '@tauri-apps/api/event'
import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useEncryptionSessionState } from '../useEncryptionSessionState'
import { getEncryptionSessionStatus } from '@/api/security'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

vi.mock('@/api/security', () => ({
  getEncryptionSessionStatus: vi.fn(),
}))

const mockListen = vi.mocked(listen)
const mockGetEncryptionSessionStatus = vi.mocked(getEncryptionSessionStatus)

describe('useEncryptionSessionState', () => {
  let callback: ((event: { payload: unknown }) => void) | null = null

  beforeEach(() => {
    vi.clearAllMocks()
    callback = null
    mockListen.mockImplementation(async (_channel: string, cb: unknown) => {
      callback = cb as (event: { payload: unknown }) => void
      return (() => {}) as () => void
    })
  })

  it('treats uninitialized encryption as ready', async () => {
    mockGetEncryptionSessionStatus.mockResolvedValue({
      initialized: false,
      session_ready: false,
    })

    const { result } = renderHook(() => useEncryptionSessionState())

    await waitFor(() => {
      expect(result.current.encryptionReady).toBe(true)
      expect(result.current.isLocked).toBe(false)
    })
  })

  it('treats initialized but locked encryption as locked', async () => {
    mockGetEncryptionSessionStatus.mockResolvedValue({
      initialized: true,
      session_ready: false,
    })

    const { result } = renderHook(() => useEncryptionSessionState())

    await waitFor(() => {
      expect(result.current.encryptionReady).toBe(false)
      expect(result.current.isLocked).toBe(true)
    })
  })

  it('switches to ready after SessionReady event', async () => {
    mockGetEncryptionSessionStatus.mockResolvedValue({
      initialized: true,
      session_ready: false,
    })

    const { result } = renderHook(() => useEncryptionSessionState())

    await waitFor(() => {
      expect(callback).not.toBeNull()
    })

    callback?.({ payload: 'SessionReady' })

    await waitFor(() => {
      expect(result.current.encryptionReady).toBe(true)
      expect(result.current.isLocked).toBe(false)
    })
  })
})
