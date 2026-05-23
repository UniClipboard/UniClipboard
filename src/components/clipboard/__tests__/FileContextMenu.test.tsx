import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import FileContextMenu from '@/components/clipboard/FileContextMenu'
import { __resetResendActionStoreForTests } from '@/hooks/useResendAction'
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

// FileContextMenu 现在通过 useEntryDelivery 懒拉 entry source,在 menu open
// 时决定是否显示 Resend。测试里 stub 成稳定快照,默认 local 来源(其它测
// 试沿用现有可见性契约),需要测 remote/historical 隐藏时单测内 override。
const deliveryHookMock = vi.fn<(entryId: string | null) => { delivery: EntryDeliveryView | null }>()
vi.mock('@/hooks/useEntryDelivery', () => ({
  useEntryDelivery: (entryId: string | null) => deliveryHookMock(entryId),
}))

function deliveryFixture(
  source: EntryDeliveryView['source'],
  entryId = 'entry-ctx-1'
): EntryDeliveryView {
  return { entryId, source, deliveries: [] }
}

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
    deliveryHookMock.mockReset()
    // 默认 local 来源:右键菜单打开后,Resend 项可见 + 可点击。需要远端 /
    // historical 时,单测里 override 这个 mock。
    deliveryHookMock.mockReturnValue({
      delivery: deliveryFixture({ tag: 'local' }),
    })
    __resetResendActionStoreForTests()
  })

  afterEach(() => {
    vi.useRealTimers()
    __resetResendActionStoreForTests()
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

  it('shows Resend until delivery loads (lazy gate) — only suppresses once source is known remote', async () => {
    // delivery 尚未拉回时 (loading / fetch 失败) 的真实形态:hook 返回
    // `{ delivery: null }`。FileContextMenu 必须保持 Resend 可见,让后端
    // typed error 兜底,而不是把"未知 = 隐藏"演化成"loading 闪烁"。
    deliveryHookMock.mockReturnValue({ delivery: null })

    renderMenu()
    openMenu()

    const item = await screen.findByRole('menuitem', {
      name: i18n.t('clipboard.contextMenu.resend'),
    })
    expect(item).toBeInTheDocument()
  })

  it('hides the Resend menu item when entry source is remote (UX parity with HoverCard popover)', async () => {
    deliveryHookMock.mockReturnValue({
      delivery: deliveryFixture({
        tag: 'remote',
        deviceId: 'dev-other',
        deviceName: 'Alice MBP',
      }),
    })

    renderMenu()
    openMenu()

    // 等 ContextMenu 渲染出 content (radix 给最外层 role=menu),确认菜单
    // 实际打开;copy 菜单项名带 Shortcut 后缀,正则匹配不够稳,改用 menu
    // 容器作锚点。
    await screen.findByRole('menu')
    expect(
      screen.queryByRole('menuitem', { name: i18n.t('clipboard.contextMenu.resend') })
    ).toBeNull()
  })

  it('hides the Resend menu item when entry source is historical', async () => {
    // legacy entry 没有追踪意义,resend 也无目标。EntryDeliveryBadge 在
    // HoverCard popover 上对 historical 隐藏整段同步区,这里对齐。
    deliveryHookMock.mockReturnValue({
      delivery: deliveryFixture({ tag: 'historical' }),
    })

    renderMenu()
    openMenu()

    await screen.findByRole('menu')
    expect(
      screen.queryByRole('menuitem', { name: i18n.t('clipboard.contextMenu.resend') })
    ).toBeNull()
  })

  it('does NOT call useEntryDelivery until the menu is opened (avoids list-level IPC fan-out)', () => {
    renderMenu()
    // 未触发右键 → useEntryDelivery 收到 null,不应发起 IPC。这条契约
    // 保护列表初始渲染的零成本承诺。
    expect(deliveryHookMock).toHaveBeenCalledWith(null)
    expect(deliveryHookMock).not.toHaveBeenCalledWith('entry-ctx-1')
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
      // entryId 走 typed envelope (commit G):toast 文案里要锚定具体哪条 entry,
      // 多条同时报错时用户不至于失去上下文。
      entryId: 'entry-remote-uuid',
      reason: 'remoteOrigin',
    })

    renderMenu()
    openMenu()

    fireEvent.click(
      await screen.findByRole('menuitem', { name: i18n.t('clipboard.contextMenu.resend') })
    )

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        i18n.t('delivery.resend.error.notResendable.remoteOrigin', {
          entryIdShort: 'entry-re…',
        })
      )
    })
    expect(toastSuccessMock).not.toHaveBeenCalled()
  })
})
