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

vi.mock('@/hooks/useClipboardPreviewState', () => ({
  useClipboardPreviewState: () => ({
    effectiveStatus: 'completed',
    entryStatus: undefined,
    imageDimensions: null,
    loading: false,
    preview: null,
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

vi.mock('@/components/clipboard/EntryDeliveryBadge', () => ({
  default: () => 'delivery-badge',
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
    expect(screen.getByText('delivery-badge')).toBeInTheDocument()
    // A remote entry cannot be re-sent from this device.
    expect(screen.getByRole('button', { name: /history\.detail\.sendToDevice/ })).toBeDisabled()
  })
})
