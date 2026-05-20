import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import FileContextMenu from '@/components/clipboard/FileContextMenu'
import i18n from '@/i18n'

// Redux store hooks 与 file transfer 选择器: FileContextMenu 在内部用 redux
// 决定 sync/copy 是否 disable;本测专注 Resend 菜单项的可达性 + 点击行为,
// 因此 selector 全 stub 成"已下载且完成"的稳定快照。
vi.mock('@/store/hooks', () => ({
  useAppSelector: (selector: (state: unknown) => unknown) => selector({}),
}))

vi.mock('@/store/slices/fileTransferSlice', () => ({
  resolveEntryTransferStatus: vi.fn(() => 'completed'),
  selectEntryTransferStatus: vi.fn(() => undefined),
  selectTransferByEntryId: vi.fn(() => undefined),
}))

// useResendAction 通过 useResendAction-internal 的真实 hook + 我们 stub 掉
// `resendEntry` API + sonner toast。这条链最贴近产线行为,只屏蔽真正会出
// 网的副作用。
const resendEntryMock = vi.fn()
const toastSuccessMock = vi.fn()
const toastErrorMock = vi.fn()

vi.mock('@/api/tauri-command/clipboard_delivery', async () => {
  const actual = await vi.importActual<typeof import('@/api/tauri-command/clipboard_delivery')>(
    '@/api/tauri-command/clipboard_delivery'
  )
  return {
    ...actual,
    resendEntry: (...args: unknown[]) => resendEntryMock(...args),
  }
})

vi.mock('sonner', () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}))

function renderMenu(overrides: Partial<React.ComponentProps<typeof FileContextMenu>> = {}) {
  const props: React.ComponentProps<typeof FileContextMenu> = {
    itemId: 'entry-ctx-1',
    itemType: 'text',
    isDownloaded: true,
    isTransferring: false,
    isStale: false,
    onCopy: vi.fn(),
    onDelete: vi.fn(),
    onSyncToClipboard: vi.fn(),
    onOpenFileLocation: vi.fn(),
    children: <div data-testid="row">row content</div>,
    ...overrides,
  }
  return render(<FileContextMenu {...props} />)
}

function openMenu() {
  // radix-ui ContextMenu 监听原生 contextmenu 事件;testing-library 的
  // userEvent 在 jsdom 环境下 right-click 行为不稳,改用 fireEvent.
  fireEvent.contextMenu(screen.getByTestId('row'))
}

describe('FileContextMenu — Resend item', () => {
  beforeEach(() => {
    resendEntryMock.mockReset()
    toastSuccessMock.mockReset()
    toastErrorMock.mockReset()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('renders the Resend menu item alongside copy/delete', async () => {
    renderMenu()
    openMenu()

    const item = await screen.findByRole('menuitem', {
      name: i18n.t('clipboard.contextMenu.resend'),
    })
    expect(item).toBeInTheDocument()
    expect(item).not.toHaveAttribute('data-disabled')
  })

  it('calls resendEntry with entryId and null filter when clicked, then surfaces success toast', async () => {
    resendEntryMock.mockResolvedValueOnce({
      accepted: 1,
      duplicate: 0,
      offline: 0,
      errored: 0,
      pending: 0,
    })

    renderMenu({ itemId: 'entry-xyz' })
    openMenu()

    const item = await screen.findByRole('menuitem', {
      name: i18n.t('clipboard.contextMenu.resend'),
    })
    fireEvent.click(item)

    await waitFor(() => {
      expect(resendEntryMock).toHaveBeenCalledWith({
        entryId: 'entry-xyz',
        targetDeviceIds: null,
      })
    })
    await waitFor(() => {
      expect(toastSuccessMock).toHaveBeenCalledWith(
        i18n.t('delivery.resend.success.summary', { accepted: 1, total: 1 })
      )
    })
    expect(toastErrorMock).not.toHaveBeenCalled()
  })

  it('surfaces typed error toast when backend rejects (e.g. remote-origin entry)', async () => {
    resendEntryMock.mockRejectedValueOnce({
      code: 'ENTRY_NOT_RESENDABLE',
      reason: 'remoteOrigin',
    })

    renderMenu()
    openMenu()

    fireEvent.click(
      await screen.findByRole('menuitem', { name: i18n.t('clipboard.contextMenu.resend') })
    )

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        i18n.t('delivery.resend.error.notResendable.remoteOrigin')
      )
    })
    expect(toastSuccessMock).not.toHaveBeenCalled()
  })
})
