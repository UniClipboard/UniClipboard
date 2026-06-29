import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import HistoryCardContextMenu from '@/components/history/HistoryCardContextMenu'
import { __resetResendActionStoreForTests } from '@/hooks/useResendAction'
import i18n from '@/i18n'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

// Paired-device snapshot the "Send" submenu reads. Selector is invoked with a
// minimal state shaped like `{ devices: { spaceMembers } }`.
const spaceMembers = [
  {
    peerId: 'peer-a',
    deviceName: 'Desk Mac',
    pairingState: 'paired',
    lastSeenAtMs: null,
    connected: true,
    channel: 'direct' as const,
    connectionAddress: null,
  },
]

vi.mock('@/store/hooks', () => ({
  useAppSelector: (selector: (state: unknown) => unknown) =>
    selector({ devices: { spaceMembers } }),
}))

// Keep the resend pipeline real (in-flight gating) but stub the networked
// `resendEntry` + sonner toast so "Send" has no side effects.
const resendEntryMock = vi.fn()
vi.mock('@/api/tauri-command/clipboard_delivery', async () => {
  const actual = await vi.importActual<typeof import('@/api/tauri-command/clipboard_delivery')>(
    '@/api/tauri-command/clipboard_delivery'
  )
  return { ...actual, resendEntry: (...args: unknown[]) => resendEntryMock(...args) }
})

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

function textItem(overrides: Partial<DisplayClipboardItem> = {}): DisplayClipboardItem {
  return {
    id: 'entry-1',
    type: 'text',
    content: { display_text: 'hello', has_detail: false, size: 5 },
    activeTime: 0,
    isFavorited: false,
    ...overrides,
  }
}

function fileItem(): DisplayClipboardItem {
  return {
    id: 'entry-file',
    type: 'file',
    content: {
      file_names: ['report.pdf'],
      file_sizes: [10],
      file_paths: ['/tmp/report.pdf'],
      file_missing: [false],
    },
    activeTime: 0,
  }
}

function renderMenu(overrides: Partial<React.ComponentProps<typeof HistoryCardContextMenu>> = {}) {
  const props: React.ComponentProps<typeof HistoryCardContextMenu> = {
    item: textItem(),
    onCopy: vi.fn(),
    onToggleFavorite: vi.fn(),
    onDelete: vi.fn(),
    children: <div data-testid="row">row content</div>,
    ...overrides,
  }
  render(<HistoryCardContextMenu {...props} />)
  return props
}

function openMenu() {
  // radix-ui ContextMenu listens for the native contextmenu event; userEvent
  // right-click is flaky under jsdom, so fire it directly.
  fireEvent.contextMenu(screen.getByTestId('row'))
}

describe('HistoryCardContextMenu', () => {
  beforeEach(() => {
    resendEntryMock.mockReset()
    __resetResendActionStoreForTests()
  })

  afterEach(() => {
    __resetResendActionStoreForTests()
  })

  it('renders copy, favorite, send and delete actions', async () => {
    renderMenu()
    openMenu()

    expect(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.copy')),
      })
    ).toBeInTheDocument()
    expect(
      screen.getByRole('menuitem', { name: new RegExp(i18n.t('clipboard.contextMenu.favorite')) })
    ).toBeInTheDocument()
    expect(
      screen.getByRole('menuitem', { name: new RegExp(i18n.t('clipboard.contextMenu.send')) })
    ).toBeInTheDocument()
    expect(
      screen.getByRole('menuitem', { name: new RegExp(i18n.t('clipboard.contextMenu.delete')) })
    ).toBeInTheDocument()
  })

  it('invokes onCopy when copy is clicked', async () => {
    const props = renderMenu()
    openMenu()

    fireEvent.click(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.copy')),
      })
    )
    expect(props.onCopy).toHaveBeenCalledWith('entry-1')
  })

  it('shows the unfavorite label and toggles current state', async () => {
    const props = renderMenu({ item: textItem({ isFavorited: true }) })
    openMenu()

    fireEvent.click(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.unfavorite')),
      })
    )
    expect(props.onToggleFavorite).toHaveBeenCalledWith('entry-1', true)
  })

  it('omits "open file location" for non-file entries', async () => {
    renderMenu()
    openMenu()

    await screen.findByRole('menuitem', { name: new RegExp(i18n.t('clipboard.contextMenu.copy')) })
    expect(
      screen.queryByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.openFileLocation')),
      })
    ).not.toBeInTheDocument()
  })

  it('shows "open file location" for file entries with a revealable path', async () => {
    renderMenu({ item: fileItem() })
    openMenu()

    expect(
      await screen.findByRole('menuitem', {
        name: i18n.t('clipboard.contextMenu.openFileLocation'),
      })
    ).toBeInTheDocument()
  })

  it('invokes onDelete when delete is clicked', async () => {
    const props = renderMenu()
    openMenu()

    fireEvent.click(
      await screen.findByRole('menuitem', {
        name: new RegExp(i18n.t('clipboard.contextMenu.delete')),
      })
    )
    expect(props.onDelete).toHaveBeenCalledWith('entry-1')
  })
})
