import { configureStore } from '@reduxjs/toolkit'
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { describe, expect, it, vi } from 'vitest'
import HistoryCard from '@/components/history/HistoryCard'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import fileTransferReducer, { updateTransferProgress } from '@/store/slices/fileTransferSlice'

// Regression for issue #1330: the sender's directory transfer row shows status
// only (no byte percentage), because its reverse progress aliases every member
// onto one id. Single-file sends and directory *receives* keep their real
// percentage.

vi.mock('@/hooks/useEntryDelivery', () => ({
  useEntryDelivery: () => ({ delivery: null, loading: false, error: null }),
}))

vi.mock('@/hooks/useRelativeTime', () => ({
  useRelativeTime: () => 'now',
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: string | Record<string, unknown>) =>
      typeof opts === 'string' ? opts : key,
  }),
}))

const noop = vi.fn()

function fileItem(overrides: Partial<DisplayClipboardItem>): DisplayClipboardItem {
  return {
    id: 'entry-1',
    type: 'file',
    content: { file_names: ['photos'], file_sizes: [100] },
    activeTime: 1,
    ...overrides,
  } as DisplayClipboardItem
}

function renderWithTransfer(
  item: DisplayClipboardItem,
  transfer: { direction: 'Sending' | 'Receiving'; bytes: number; total: number }
) {
  const store = configureStore({ reducer: { fileTransfer: fileTransferReducer } })
  store.dispatch(
    updateTransferProgress({
      transferId: item.id,
      entryId: item.id,
      peerId: 'peer',
      direction: transfer.direction,
      bytesTransferred: transfer.bytes,
      totalBytes: transfer.total,
    })
  )
  render(
    <Provider store={store}>
      <HistoryCard
        item={item}
        isHovered={false}
        copySuccess={false}
        isDeleting={false}
        onCopy={noop}
        onDelete={noop}
        onToggleFavorite={noop}
        onClick={noop}
        onHoverChange={noop}
      />
    </Provider>
  )
}

describe('HistoryCard directory send progress (issue #1330)', () => {
  it('renders status only (no percentage) for a directory send', () => {
    renderWithTransfer(fileItem({ isDirectory: true }), {
      direction: 'Sending',
      bytes: 50,
      total: 100,
    })

    expect(screen.getByText('clipboard.transfer.transferring')).toBeInTheDocument()
    expect(screen.queryByText('50%')).not.toBeInTheDocument()
    expect(screen.queryByText('50 B / 100 B')).not.toBeInTheDocument()
  })

  it('keeps the byte percentage for a single-file send', () => {
    renderWithTransfer(fileItem({ isDirectory: false }), {
      direction: 'Sending',
      bytes: 50,
      total: 100,
    })

    expect(screen.getByText('50%')).toBeInTheDocument()
    expect(screen.queryByText('clipboard.transfer.transferring')).not.toBeInTheDocument()
  })

  it('keeps the aggregate percentage for a directory receive', () => {
    // Receiving directories aggregate real per-member progress, so their
    // percentage is meaningful and must not be suppressed.
    renderWithTransfer(fileItem({ isDirectory: true }), {
      direction: 'Receiving',
      bytes: 50,
      total: 100,
    })

    expect(screen.getByText('50%')).toBeInTheDocument()
    expect(screen.queryByText('clipboard.transfer.transferring')).not.toBeInTheDocument()
  })
})
