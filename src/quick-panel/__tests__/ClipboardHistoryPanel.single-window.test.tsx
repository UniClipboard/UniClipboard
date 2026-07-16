import { configureStore } from '@reduxjs/toolkit'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactElement } from 'react'
import { Provider } from 'react-redux'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { deleteClipboardEntry, restoreClipboardEntry } from '@/api/daemon'
import { __resetResendActionStoreForTests } from '@/hooks/useResendAction'
import i18n from '@/i18n'
import { playUiSound } from '@/lib/ui-sound'
import devicesReducer from '@/store/slices/devicesSlice'
import ClipboardHistoryPanel from '../ClipboardHistoryPanel'
import { useHistorySearch } from '../hooks/useHistorySearch'

const invokeMock = vi.fn()

// The panel now primes the paired-device list (for the row context menu's
// "send to device" submenu) and the reused `HistoryCardContextMenu` reads
// `state.devices.spaceMembers`, so the panel needs a Redux Provider. Keep it
// isolated: a minimal devices-only store plus a members API that returns none.
vi.mock('@/api/daemon/members', () => ({
  getPairedPeersWithStatus: vi.fn().mockResolvedValue([]),
  getLocalDeviceInfo: vi.fn().mockResolvedValue(null),
}))

function renderPanel(ui: ReactElement = <ClipboardHistoryPanel />) {
  const store = configureStore({ reducer: { devices: devicesReducer } })
  return render(<Provider store={store}>{ui}</Provider>)
}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@/hooks/useThemeSync', () => ({ useThemeSync: vi.fn() }))
vi.mock('@/hooks/useHistorySourceOptions', () => ({ useHistorySourceOptions: () => [] }))
vi.mock('@/hooks/useShortcut', () => ({ useShortcut: vi.fn() }))
vi.mock('@/lib/ui-sound', () => ({ playUiSound: vi.fn() }))

// The panel now sources its list from the unified live browse/search hook; mock
// it directly with the launcher's simplified DisplayItem shape (these tests
// exercise the preview pane, not the data layer).
vi.mock('../hooks/useHistorySearch', () => ({
  useHistorySearch: vi.fn(() => ({
    filteredItems: [
      {
        id: 'entry-1',
        type: 'text',
        preview: 'Preview title',
        activeTime: Date.now(),
        isUnavailable: false,
      },
      {
        id: 'entry-2',
        type: 'text',
        preview: 'Second preview title',
        activeTime: Date.now() - 1000,
        isUnavailable: false,
      },
      {
        id: 'entry-file',
        type: 'file',
        preview: 'File preview title',
        activeTime: Date.now() - 2000,
        isUnavailable: false,
      },
    ],
    previewItems: [
      {
        id: 'entry-1',
        type: 'text',
        content: { display_text: 'Preview title', has_detail: false, size: 13 },
        activeTime: Date.now(),
        isUnavailable: false,
      },
      {
        id: 'entry-2',
        type: 'text',
        content: { display_text: 'Second preview title', has_detail: false, size: 20 },
        activeTime: Date.now() - 1000,
        isUnavailable: false,
      },
      {
        id: 'entry-file',
        type: 'file',
        content: {
          file_names: ['report.pdf'],
          file_sizes: [10],
          file_paths: ['/tmp/report.pdf'],
          file_missing: [false],
        },
        activeTime: Date.now() - 2000,
        isUnavailable: false,
      },
    ],
    isSearching: false,
    searchTotal: 2,
    loading: false,
    isLocked: false,
    removeItem: vi.fn(),
  })),
}))

vi.mock('@/api/daemon', () => ({
  restoreClipboardEntry: vi.fn(),
  deleteClipboardEntry: vi.fn(),
  getEncryptionState: vi.fn().mockResolvedValue({ initialized: false, sessionReady: false }),
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
  // Backs favoriteClipboardItem/unfavoriteClipboardItem so the favorite toggle
  // resolves and the optimistic star sticks.
  toggleFavorite: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/api/daemon/client', () => ({
  daemonClient: {
    fetchBlob: vi.fn(),
    callSdk: vi.fn().mockResolvedValue({ data: [], ts: 1710000000000 }),
  },
}))

vi.mock('../ClipboardPreviewPane', () => ({
  default: ({ item }: { item: { id: string } | null }) =>
    item ? <div>{`Preview for ${item.id}`}</div> : <div data-testid="preview-empty" />,
}))

// The preview opens on a PREVIEW_OPEN_DELAY_MS (500ms) timer, so anything that
// asserts post-open layout has to wait for the state to land. Sleeping just past
// the delay is not enough: the timer is rescheduled by the renders that follow
// mount, which pushes the flip to ~520ms and leaves a loaded CI runner far too
// little headroom. Wait on the condition with a timeout well clear of the delay.
const PREVIEW_OPEN_TIMEOUT_MS = 3000

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
    renderPanel()

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
    const { container } = renderPanel()

    expect(
      await screen.findByText('Preview for entry-1', undefined, {
        timeout: PREVIEW_OPEN_TIMEOUT_MS,
      })
    ).toBeInTheDocument()

    const rootLayout = container.firstElementChild as HTMLDivElement | null
    const historyWrapper = rootLayout?.children.item(0) as HTMLDivElement | null
    const previewWrapper = rootLayout?.children.item(1) as HTMLDivElement | null

    // The panes are briefly pinned while the resize is in flight; the flexible
    // layout is the settled end state, so assert only once the expand lands.
    await waitFor(() => expect(previewWrapper?.className).toContain('flex-1'), {
      timeout: PREVIEW_OPEN_TIMEOUT_MS,
    })

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

    const { container } = renderPanel()
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

    const previewWrapper = rootLayout?.children.item(1) as HTMLDivElement | null

    // `pendingResize` never settles here, so the reserving state is stable once
    // reached — waiting for it is safe, and does not race the open delay.
    await waitFor(() => expect(historyWrapper?.className).toContain('shrink-0'), {
      timeout: PREVIEW_OPEN_TIMEOUT_MS,
    })

    expect(previewWrapper?.getAttribute('aria-hidden')).toBe('true')
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
    renderPanel()

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
    const { container } = renderPanel()

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
    renderPanel()

    const secondItem = await screen.findByText('Second preview title')

    fireEvent.mouseEnter(secondItem)

    // The selection preview (entry-1) still wins: a mouseEnter without a prior
    // mouseMove is not a hover, so entry-2 never becomes the preview target.
    expect(
      await screen.findByText('Preview for entry-1', undefined, {
        timeout: PREVIEW_OPEN_TIMEOUT_MS,
      })
    ).toBeInTheDocument()
    expect(screen.queryByText('Preview for entry-2')).not.toBeInTheDocument()
  })
})

describe('ClipboardHistoryPanel row context menu', () => {
  beforeEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
    invokeMock.mockResolvedValue(undefined)
    Element.prototype.scrollIntoView = vi.fn()
    // Shared module-level in-flight store — reset so the "Send" trigger renders
    // enabled and state doesn't leak between cases.
    __resetResendActionStoreForTests()
  })

  // radix-ui ContextMenu listens for the native contextmenu event; fire it
  // directly (userEvent right-click is flaky under jsdom).
  function openRowMenu(rowText: string) {
    fireEvent.contextMenu(screen.getByText(rowText))
  }

  it('selects the right-clicked row so the highlight tracks the menu target', async () => {
    renderPanel()

    const firstRow = screen.getByText('Preview title').closest('[role="option"]')
    const secondRow = screen.getByText('Second preview title').closest('[role="option"]')
    // Panel opens with the first row selected.
    expect(firstRow).toHaveAttribute('aria-selected', 'true')
    expect(secondRow).toHaveAttribute('aria-selected', 'false')

    // Right-clicking the second row moves the selection onto it.
    openRowMenu('Second preview title')

    await waitFor(() => {
      expect(secondRow).toHaveAttribute('aria-selected', 'true')
    })
    expect(firstRow).toHaveAttribute('aria-selected', 'false')
  })

  it('opens the reused history menu on right-click', async () => {
    renderPanel()
    openRowMenu('Preview title')

    expect(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.copy')),
      })
    ).toBeInTheDocument()
    expect(
      screen.getByRole('menuitem', { name: new RegExp(i18n.t('clipboard.contextMenu.delete')) })
    ).toBeInTheDocument()
  })

  it('copies the entry then dismisses the panel when Copy is clicked', async () => {
    renderPanel()
    openRowMenu('Preview title')

    fireEvent.click(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.copy')),
      })
    )

    await waitFor(() => {
      expect(restoreClipboardEntry).toHaveBeenCalledWith('entry-1')
      expect(playUiSound).toHaveBeenCalledWith('success')
    })
    // Copy is a terminal launcher action: the panel dismisses only on success.
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('dismiss_quick_panel', expect.any(Object))
    })
  })

  it('pastes file paths from the file entry context menu', async () => {
    renderPanel()
    openRowMenu('File preview title')

    fireEvent.click(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.pasteFilePaths')),
      })
    )

    await waitFor(() => {
      expect(restoreClipboardEntry).toHaveBeenCalledWith('entry-file', { filePathsOnly: true })
      expect(invokeMock).toHaveBeenCalledWith('paste_to_previous_app', expect.any(Object))
      expect(playUiSound).toHaveBeenCalledWith('success')
    })
  })

  it('keeps copy failures silent', async () => {
    vi.mocked(restoreClipboardEntry).mockRejectedValueOnce(new Error('copy failed'))
    renderPanel()
    openRowMenu('Preview title')

    fireEvent.click(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.copy')),
      })
    )

    await waitFor(() => {
      expect(restoreClipboardEntry).toHaveBeenCalledWith('entry-1')
    })
    expect(playUiSound).not.toHaveBeenCalled()
  })

  it('shows a favorite star on the row after Favorite is clicked', async () => {
    renderPanel()

    const row = screen.getByText('Preview title').closest('[role="option"]') as HTMLElement
    // No star before favoriting.
    expect(row.querySelector('.lucide-star')).toBeNull()

    openRowMenu('Preview title')
    fireEvent.click(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.favorite')),
      })
    )

    // Optimistic star appears immediately, giving the toggle visible feedback.
    await waitFor(() => {
      expect(row.querySelector('.lucide-star')).not.toBeNull()
    })
  })

  it('deletes the entry immediately (no confirm) when Delete is clicked', async () => {
    renderPanel()
    openRowMenu('Preview title')

    fireEvent.click(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.delete')),
      })
    )

    await waitFor(() => {
      expect(deleteClipboardEntry).toHaveBeenCalledWith('entry-1')
    })
    // Immediate delete does not route through a confirmation dialog.
    expect(invokeMock).not.toHaveBeenCalledWith('dismiss_quick_panel', expect.any(Object))
  })
})

describe('ClipboardHistoryPanel hover/focus keyboard shortcuts', () => {
  beforeEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
    invokeMock.mockResolvedValue(undefined)
    Element.prototype.scrollIntoView = vi.fn()
  })

  function searchBox() {
    return screen.getByRole('combobox') as HTMLInputElement
  }

  // The `searchQuery` the data layer was last asked for. Proxies "what the list
  // is actually showing" without unmocking the data layer.
  function lastHistoryQuery() {
    const calls = vi.mocked(useHistorySearch).mock.calls
    return calls.at(-1)?.[0]?.searchQuery
  }

  it('acts on the hovered row rather than the keyboard-selected row when both differ', async () => {
    renderPanel()
    // Panel opens with entry-1 selected by keyboard.
    fireEvent.mouseMove(screen.getByText('Second preview title'))
    fireEvent.mouseEnter(screen.getByText('Second preview title'))
    await screen.findByText('Preview for entry-2')

    fireEvent.keyDown(searchBox(), { key: 'c', metaKey: true })

    await waitFor(() => {
      expect(restoreClipboardEntry).toHaveBeenCalledWith('entry-2')
    })
  })

  it('copies the active row to the clipboard and dismisses the panel on Ctrl/Cmd+C', async () => {
    renderPanel()

    fireEvent.keyDown(searchBox(), { key: 'c', metaKey: true })

    await waitFor(() => {
      expect(restoreClipboardEntry).toHaveBeenCalledWith('entry-1')
    })
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('dismiss_quick_panel', expect.any(Object))
    })
  })

  it('does not hijack Ctrl/Cmd+C when the search input has a text selection', async () => {
    renderPanel()
    const input = searchBox()
    fireEvent.change(input, { target: { value: 'abc' } })
    input.setSelectionRange(0, 3)

    fireEvent.keyDown(input, { key: 'c', metaKey: true })

    expect(restoreClipboardEntry).not.toHaveBeenCalled()
  })

  it('pastes the active row to the foreground app on Ctrl/Cmd+V, same as Enter', async () => {
    renderPanel()

    fireEvent.keyDown(searchBox(), { key: 'v', metaKey: true })

    await waitFor(() => {
      expect(restoreClipboardEntry).toHaveBeenCalledWith('entry-1', undefined)
    })
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('paste_to_previous_app', expect.any(Object))
    })
  })

  it('does not hijack Ctrl/Cmd+V once the search query is non-empty', async () => {
    renderPanel()
    const input = searchBox()
    fireEvent.change(input, { target: { value: 'abc' } })

    fireEvent.keyDown(input, { key: 'v', metaKey: true })

    expect(restoreClipboardEntry).not.toHaveBeenCalled()
  })

  it('deletes the active row on Alt+Backspace', async () => {
    renderPanel()

    fireEvent.keyDown(searchBox(), { key: 'Backspace', altKey: true })

    await waitFor(() => {
      expect(deleteClipboardEntry).toHaveBeenCalledWith('entry-1')
    })
  })

  it('remounts a fresh session on re-open so a settled prior query does not carry over', async () => {
    // The outer shell keys the session on showRequestId: buffer, filters, and
    // the live-search query all start empty instead of flashing the last show.
    const store = configureStore({ reducer: { devices: devicesReducer } })
    const { rerender } = render(
      <Provider store={store}>
        <ClipboardHistoryPanel showRequestId={1} />
      </Provider>
    )
    // Let the first show settle into an active 'invoice' query (past the
    // debounce), so the remount has a real prior query to discard.
    fireEvent.change(searchBox(), { target: { value: 'invoice' } })
    await waitFor(() => expect(lastHistoryQuery()).toBe('invoice'))
    expect(searchBox()).toHaveValue('invoice')

    rerender(
      <Provider store={store}>
        <ClipboardHistoryPanel showRequestId={2} />
      </Provider>
    )

    // Both the box and the query the data layer sees reset to empty; the list
    // never re-queries with the previous show's 'invoice'.
    await waitFor(() => expect(searchBox()).toHaveValue(''))
    expect(lastHistoryQuery()).toBe('')
  })

  it('debounces typing but sends the cleared query to the data layer immediately', async () => {
    renderPanel()
    const input = searchBox()

    // Typing does not reach the data layer synchronously — it waits for the
    // debounce, so the browse query stays empty until the timer fires.
    fireEvent.change(input, { target: { value: 'abc' } })
    expect(lastHistoryQuery()).toBe('')
    await waitFor(() => expect(lastHistoryQuery()).toBe('abc'))

    // Clearing bypasses the debounce: the empty browse query is applied on the
    // next render, not ~300ms later, so the list stops matching 'abc' at once.
    fireEvent.change(input, { target: { value: '' } })
    expect(lastHistoryQuery()).toBe('')
  })
})
