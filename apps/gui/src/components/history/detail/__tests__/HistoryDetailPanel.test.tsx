import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import HistoryDetailPanel from '../HistoryDetailPanel'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: unknown) =>
      typeof opts === 'object' && opts !== null && 'count' in opts
        ? `${key}:${String(opts.count)}`
        : key,
    i18n: { language: 'en-US' },
  }),
  Trans: ({ i18nKey, values }: { i18nKey: string; values: { device: string } }) =>
    `${i18nKey}:${values.device}`,
}))

const dimsMock = vi.hoisted((): { value: { width: number; height: number } | null } => ({
  value: null,
}))
const previewMock = vi.hoisted((): { value: unknown } => ({ value: null }))
vi.mock('@/hooks/useClipboardPreviewState', () => ({
  useClipboardPreviewState: () => ({
    effectiveStatus: 'completed',
    entryStatus: undefined,
    imageDimensions: dimsMock.value,
    loading: false,
    preview: previewMock.value,
    setImageDimensions: vi.fn(),
    transfer: undefined,
  }),
}))

const deliveryMock = vi.hoisted((): { value: EntryDeliveryView | null } => ({ value: null }))
vi.mock('@/hooks/useEntryDelivery', () => ({
  useEntryDelivery: () => ({ delivery: deliveryMock.value, loading: false, error: null }),
}))

vi.mock('@/components/clipboard/ClipboardPreview', () => ({
  PreviewContent: ({ item }: { item: DisplayClipboardItem }) => `preview:${item.id}`,
}))

vi.mock('@/components/clipboard/ClipboardSendMenu', () => ({
  default: ({
    disabled,
    renderTrigger,
  }: {
    disabled?: boolean
    renderTrigger: (state: { disabled: boolean; busy: boolean }) => React.ReactElement
  }) => renderTrigger({ disabled: Boolean(disabled), busy: false }),
}))

const resendMock = vi.hoisted(() => ({
  resendAll: vi.fn(),
  resendToPeer: vi.fn(),
}))
vi.mock('@/hooks/useResendAction', () => ({
  useResendAction: () => ({
    ...resendMock,
    isEntryInFlight: () => false,
    isPeerInFlight: () => false,
  }),
}))

const shortcutMock = vi.hoisted(() => ({
  handlers: new Map<string, () => void>(),
}))
vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: ({
    key,
    enabled = true,
    handler,
  }: {
    key: string
    enabled?: boolean
    handler: () => void
  }) => {
    if (enabled) shortcutMock.handlers.set(key, handler)
    else shortcutMock.handlers.delete(key)
  },
}))

const storageMock = vi.hoisted(() => ({
  saveImageAs: vi.fn(async () => '/tmp/out.png'),
  openImageExternally: vi.fn(async () => undefined),
}))
vi.mock('@/api/storage', () => storageMock)

vi.mock('@/lib/image-handoff', () => ({
  imageFormatLabel: (mime: string | null) => (mime === 'image/png' ? 'PNG' : null),
  imageFileName: (base: string) => `${base}.png`,
  loadImageBlob: async () => new Blob([new Uint8Array([1, 2, 3])], { type: 'image/png' }),
}))

vi.mock('@/hooks/useBlobImageObjectUrl', () => ({
  useBlobImageObjectUrl: (descriptor: string | null, enabled: boolean) =>
    descriptor && enabled ? 'blob:full' : null,
}))

vi.mock('@/api/file_transfer', () => ({
  cancelEntryReceive: vi.fn(),
  cancelFileTransfer: vi.fn(),
}))

const item: DisplayClipboardItem = {
  id: 'entry-1',
  type: 'text',
  contentTags: ['code'],
  content: { display_text: 'docker push', has_detail: false, size: 11, char_count: 11 },
  activeTime: new Date(2026, 8, 27, 22, 14, 8).getTime(),
  isFavorited: true,
}

function renderPanel(overrides: Partial<DisplayClipboardItem> | null = {}) {
  const handlers = { onCopy: vi.fn(), onToggleFavorite: vi.fn(), onDelete: vi.fn() }
  render(
    <HistoryDetailPanel
      item={overrides === null ? null : { ...item, ...overrides }}
      copySuccess={false}
      {...handlers}
    />
  )
  return handlers
}

describe('HistoryDetailPanel', () => {
  beforeEach(() => {
    deliveryMock.value = null
    previewMock.value = null
    dimsMock.value = null
    shortcutMock.handlers.clear()
    storageMock.saveImageAs.mockClear()
    storageMock.openImageExternally.mockClear()
  })

  it('shows the empty state when nothing is selected', () => {
    renderPanel(null)

    expect(screen.getByText('history.detail.emptyTitle')).toBeInTheDocument()
    expect(screen.getByText('history.detail.emptyHint')).toBeInTheDocument()
  })

  it('renders the kind pill, content and copy facts', () => {
    renderPanel()

    expect(screen.getByText('history.type.code')).toBeInTheDocument()
    expect(screen.getByText('preview:entry-1')).toBeInTheDocument()
    expect(screen.getByText('Sep 27 · 22:14:08')).toBeInTheDocument()
    expect(screen.getByText('clipboard.preview.charactersCount:11')).toBeInTheDocument()
  })

  it('wires pin, delete, copy and send', async () => {
    const user = userEvent.setup()
    const handlers = renderPanel()

    const unpin = screen.getByRole('button', { name: 'history.detail.unpin' })
    expect(unpin).toHaveAttribute('aria-pressed', 'true')
    await user.click(unpin)
    await user.click(screen.getByRole('button', { name: 'clipboard.actionBar.delete' }))
    await user.click(screen.getByRole('button', { name: 'clipboard.actionBar.copy' }))

    expect(handlers.onToggleFavorite).toHaveBeenCalledTimes(1)
    expect(handlers.onDelete).toHaveBeenCalledTimes(1)
    expect(handlers.onCopy).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('button', { name: /history\.detail\.sendToDevice/ })).toBeEnabled()
  })

  it('names the source device and lists per-device delivery', () => {
    deliveryMock.value = {
      entryId: 'entry-1',
      source: { tag: 'remote', deviceId: 'dev-iphone', deviceName: 'iPhone 16' },
      deliveries: [
        {
          targetDeviceId: 'dev-pixel-000000',
          targetDeviceName: null,
          status: { tag: 'unreachable' },
          reasonDetail: null,
          updatedAtMs: null,
        },
      ],
    }
    renderPanel()

    expect(screen.getByText('history.detail.from:iPhone 16')).toBeInTheDocument()
    expect(screen.getByText('dev-pixe…')).toBeInTheDocument()
    expect(screen.getByText('delivery.status.unreachable')).toBeInTheDocument()
    expect(screen.getByText('delivery.summary.waiting')).toBeInTheDocument()
    // A remote entry offers no resend, even for an offline peer.
    expect(screen.queryByRole('button', { name: /delivery\.resend/ })).not.toBeInTheDocument()
    // A remote entry cannot be re-sent from this device.
    expect(screen.getByRole('button', { name: /history\.detail\.sendToDevice/ })).toBeDisabled()
  })

  it('resends a failed peer from its chip and keeps delivered ones quiet', async () => {
    const user = userEvent.setup()
    deliveryMock.value = {
      entryId: 'entry-1',
      source: { tag: 'local' },
      deliveries: [
        {
          targetDeviceId: 'dev-mac',
          targetDeviceName: 'MacBook',
          status: { tag: 'delivered' },
          reasonDetail: null,
          updatedAtMs: null,
        },
        {
          targetDeviceId: 'dev-pc',
          targetDeviceName: 'Office PC',
          status: { tag: 'failed', reason: 'io' },
          reasonDetail: null,
          updatedAtMs: null,
        },
      ],
    }
    renderPanel()

    expect(screen.getByText('delivery.summary.partial')).toBeInTheDocument()
    expect(screen.getByText('MacBook')).toBeInTheDocument()
    expect(screen.queryByText('delivery.status.delivered')).not.toBeInTheDocument()
    expect(
      screen.getAllByRole('button', { name: /delivery\.resend\.button\.peerAria/ })
    ).toHaveLength(1)

    await user.click(screen.getByRole('button', { name: /delivery\.resend\.button\.peerAria/ }))
    expect(resendMock.resendToPeer).toHaveBeenCalledWith('entry-1', 'dev-pc')
    await user.click(screen.getByRole('button', { name: 'delivery.resend.button.entryAria' }))
    expect(resendMock.resendAll).toHaveBeenCalledWith('entry-1')
  })

  describe('image entry', () => {
    const imageItem: Partial<DisplayClipboardItem> = {
      type: 'image',
      contentTags: [],
      content: { size: 2048, width: 800, height: 600 },
    }

    beforeEach(() => {
      previewMock.value = {
        contentType: 'image',
        entryId: 'entry-1',
        sizeBytes: 2048,
        imageBlobPath: '/clipboard/blobs/b1',
      }
    })

    it('shows dimensions, format and size even when the search result has no image content', () => {
      previewMock.value = {
        contentType: 'image',
        entryId: 'entry-1',
        sizeBytes: 2048,
        mimeType: 'image/png',
        imageBlobPath: '/clipboard/blobs/b1',
      }
      dimsMock.value = { width: 2880, height: 1800 }
      renderPanel({ ...imageItem, content: null })

      expect(screen.getByText('2880 × 1800')).toBeInTheDocument()
      expect(screen.getByText('PNG · 2.00 KB')).toBeInTheDocument()
      expect(screen.getByText('history.detail.copied')).toBeInTheDocument()
    })

    it('hands the image bytes to Save as and the default viewer', async () => {
      const user = userEvent.setup()
      renderPanel(imageItem)

      await user.click(screen.getByRole('button', { name: 'history.detail.saveAs' }))
      await vi.waitFor(() => expect(storageMock.saveImageAs).toHaveBeenCalledTimes(1))
      const [fileName, bytes] = storageMock.saveImageAs.mock.calls[0] as unknown as [
        string,
        Uint8Array,
      ]
      expect(fileName).toBe('history.detail.imageFileName.png')
      expect([...bytes]).toEqual([1, 2, 3])

      await user.click(screen.getByRole('button', { name: 'history.detail.openInPreview' }))
      await vi.waitFor(() => expect(storageMock.openImageExternally).toHaveBeenCalledTimes(1))
    })

    it('switches between Fit and 100%', async () => {
      const user = userEvent.setup()
      renderPanel(imageItem)

      const fit = screen.getByRole('button', { name: 'history.detail.imageFit' })
      const actual = screen.getByRole('button', { name: 'history.detail.imageActual' })
      expect(fit).toHaveAttribute('aria-pressed', 'true')
      await user.click(actual)
      expect(actual).toHaveAttribute('aria-pressed', 'true')
      expect(fit).toHaveAttribute('aria-pressed', 'false')
    })

    it('opens Quick Look from the button and closes it with its close button', async () => {
      const user = userEvent.setup()
      renderPanel(imageItem)

      expect(screen.queryByTestId('image-quick-look')).not.toBeInTheDocument()
      await user.click(screen.getByRole('button', { name: /history\.detail\.quickLook/ }))
      expect(screen.getByTestId('image-quick-look').querySelector('img')).toHaveAttribute(
        'src',
        'blob:full'
      )
      await user.click(screen.getByRole('button', { name: 'history.detail.quickLookClose' }))
      expect(screen.queryByTestId('image-quick-look')).not.toBeInTheDocument()
    })

    it('registers Space for Quick Look only for images', () => {
      renderPanel(imageItem)
      expect(shortcutMock.handlers.has('space')).toBe(true)
    })
  })

  it('offers no image actions for other kinds', () => {
    renderPanel()

    expect(shortcutMock.handlers.has('space')).toBe(false)
    expect(screen.queryByRole('button', { name: 'history.detail.saveAs' })).not.toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: /history\.detail\.quickLook/ })
    ).not.toBeInTheDocument()
  })
})
