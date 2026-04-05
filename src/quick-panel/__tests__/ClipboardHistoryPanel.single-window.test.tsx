import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import ClipboardHistoryPanel from '../ClipboardHistoryPanel'

const invokeMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@/hooks/useThemeSync', () => ({ useThemeSync: vi.fn() }))

vi.mock('@/hooks/useClipboardCollection', () => ({
  useClipboardCollection: vi.fn(() => ({
    items: [
      {
        id: 'entry-1',
        is_downloaded: true,
        is_favorited: false,
        created_at: 1710000000000,
        updated_at: 1710000000000,
        active_time: Date.now(),
        item: {
          text: {
            display_text: 'Preview title',
            has_detail: true,
            size: 13,
          },
          image: null,
          file: null,
          link: null,
          code: null,
          unknown: null,
        },
        file_transfer_status: null,
        file_transfer_reason: null,
      },
    ],
    loading: false,
    isLocked: false,
    reload: vi.fn(),
  })),
}))

vi.mock('@/api/daemon', () => ({
  restoreClipboardEntry: vi.fn(),
  deleteClipboardEntry: vi.fn(),
}))

vi.mock('@/api/security', () => ({
  unlockEncryptionSession: vi.fn(),
}))

vi.mock('@/api/daemon/clipboard', () => ({
  getClipboardEntryResource: vi.fn().mockResolvedValue({
    blobId: null,
    mimeType: 'text/plain',
    sizeBytes: 17,
    url: null,
    inlineData: null,
  }),
  getClipboardEntryDetail: vi.fn().mockResolvedValue({
    id: 'entry-1',
    content: 'Full preview text',
    sizeBytes: 17,
    createdAtMs: 1710000000000,
    activeTimeMs: 1710000000000,
    mimeType: 'text/plain',
  }),
}))

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    blobUrl: vi.fn((path: string) => `http://127.0.0.1:12345${path}?auth=Session+test`),
  },
}))

describe('ClipboardHistoryPanel single-window preview', () => {
  beforeEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
    invokeMock.mockResolvedValue(undefined)
    Element.prototype.scrollIntoView = vi.fn()
  })

  it('renders preview content inside the same panel and requests expanded mode', async () => {
    render(<ClipboardHistoryPanel />)

    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 550))
    })

    expect(await screen.findByText('Full preview text')).toBeInTheDocument()

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('set_quick_panel_preview_expanded', {
        expanded: true,
      })
    })
    expect(invokeMock).not.toHaveBeenCalledWith('show_preview_panel', expect.anything())
  })

  it('dismisses the quick window immediately when escape is pressed with preview open', async () => {
    render(<ClipboardHistoryPanel />)

    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 550))
    })

    expect(await screen.findByText('Full preview text')).toBeInTheDocument()

    invokeMock.mockClear()

    fireEvent.keyDown(window, { key: 'Escape' })

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('dismiss_quick_panel')
    })
    expect(invokeMock).not.toHaveBeenCalledWith('set_quick_panel_preview_expanded', {
      expanded: false,
    })
  })
})
