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
      {
        id: 'entry-2',
        is_downloaded: true,
        is_favorited: false,
        created_at: 1710000001000,
        updated_at: 1710000001000,
        active_time: Date.now() - 1000,
        item: {
          text: {
            display_text: 'Second preview title',
            has_detail: true,
            size: 19,
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
    inlineData: btoa('Full preview text'),
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

vi.mock('../ClipboardPreviewPane', () => ({
  default: ({ entryId }: { entryId: string | null }) =>
    entryId ? <div>{`Preview for ${entryId}`}</div> : <div data-testid="preview-empty" />,
}))

function deferred() {
  let resolve!: () => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<void>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

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

    expect(await screen.findByText('Preview for entry-1')).toBeInTheDocument()

    await waitFor(() => {
      // 现在走 typed `commands` proxy → generated bindings → 注入 trace 字段，
      // 所以 invoke 收到的 payload 多一个 `trace`。用 objectContaining 匹配。
      expect(invokeMock).toHaveBeenCalledWith(
        'set_quick_panel_layout',
        expect.objectContaining({ scale: 1, previewExpanded: true })
      )
    })
    expect(invokeMock).not.toHaveBeenCalledWith('show_preview_panel', expect.anything())
  })

  it('keeps history and preview panes flexible when the inline preview opens', async () => {
    const { container } = render(<ClipboardHistoryPanel />)

    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 550))
    })

    expect(await screen.findByText('Preview for entry-1')).toBeInTheDocument()

    const rootLayout = container.firstElementChild as HTMLDivElement | null
    const historyWrapper = rootLayout?.children.item(0) as HTMLDivElement | null
    const previewWrapper = rootLayout?.children.item(1) as HTMLDivElement | null

    expect(historyWrapper?.className).toContain('basis-0')
    expect(historyWrapper?.className).toContain('min-w-0')

    expect(previewWrapper?.className).toContain('basis-0')
    expect(previewWrapper?.className).toContain('flex-1')
    expect(previewWrapper?.className).not.toContain('transition-all')
    expect(previewWrapper?.firstElementChild?.className).toContain('transition-[opacity,transform]')
  })

  it('waits for the backend layout resize before expanding the preview column', async () => {
    const pendingResize = deferred()
    invokeMock.mockImplementation((command: string, payload?: { previewExpanded?: boolean }) => {
      if (command === 'set_quick_panel_layout' && payload?.previewExpanded) {
        return pendingResize.promise
      }
      return Promise.resolve(undefined)
    })

    const { container } = render(<ClipboardHistoryPanel />)
    const rootLayout = container.firstElementChild as HTMLDivElement | null
    const historyWrapper = rootLayout?.children.item(0) as HTMLDivElement | null
    historyWrapper!.getBoundingClientRect = vi.fn(() => ({
      width: 360,
      height: 420,
      top: 0,
      right: 360,
      bottom: 420,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    }))

    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 550))
    })

    const previewWrapper = rootLayout?.children.item(1) as HTMLDivElement | null

    expect(previewWrapper?.getAttribute('aria-hidden')).toBe('true')
    expect(historyWrapper?.className).toContain('shrink-0')
    expect(historyWrapper?.style.width).toBe('360px')
    expect(previewWrapper?.className).toContain('shrink-0')

    await act(async () => {
      pendingResize.resolve()
      await Promise.resolve()
    })

    expect(previewWrapper?.getAttribute('aria-hidden')).toBe('false')
    expect(historyWrapper?.className).toContain('flex-1')
    expect(historyWrapper?.style.width).toBe('')
    expect(previewWrapper?.className).toContain('flex-1')
    expect(previewWrapper?.style.width).toBe('')
  })

  it('dismisses the quick window immediately when escape is pressed with preview open', async () => {
    render(<ClipboardHistoryPanel />)

    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 550))
    })

    expect(await screen.findByText('Preview for entry-1')).toBeInTheDocument()

    invokeMock.mockClear()

    fireEvent.keyDown(window, { key: 'Escape' })

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('dismiss_quick_panel', expect.any(Object))
    })
    expect(invokeMock).not.toHaveBeenCalledWith(
      'set_quick_panel_layout',
      expect.objectContaining({ scale: 1, previewExpanded: false })
    )
  })

  it('keeps the hovered preview when moving from history into the preview pane', async () => {
    const { container } = render(<ClipboardHistoryPanel />)

    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 550))
    })

    const rootLayout = container.firstElementChild as HTMLDivElement | null
    const previewWrapper = rootLayout?.children.item(1) as HTMLDivElement | null
    const secondItem = screen.getByText('Second preview title')

    fireEvent.mouseMove(secondItem)
    fireEvent.mouseEnter(secondItem)

    expect(await screen.findByText('Preview for entry-2')).toBeInTheDocument()

    fireEvent.mouseLeave(secondItem)
    fireEvent.mouseEnter(previewWrapper!)

    expect(screen.getByText('Preview for entry-2')).toBeInTheDocument()
    expect(screen.queryByText('Preview for entry-1')).not.toBeInTheDocument()
  })

  it('does not treat a stationary pointer as a hover when the panel first appears', async () => {
    render(<ClipboardHistoryPanel />)

    const secondItem = await screen.findByText('Second preview title')

    fireEvent.mouseEnter(secondItem)

    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 550))
    })

    expect(screen.getByText('Preview for entry-1')).toBeInTheDocument()
    expect(screen.queryByText('Preview for entry-2')).not.toBeInTheDocument()
  })
})
