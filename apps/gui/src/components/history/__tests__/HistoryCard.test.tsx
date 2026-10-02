import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import HistoryCard from '@/components/history/HistoryCard'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

vi.mock('@/hooks/useEntryDelivery', () => ({
  useEntryDelivery: () => ({ delivery: null, loading: false, error: null }),
}))

vi.mock('@/hooks/useRelativeTime', () => ({
  useRelativeTime: () => 'now',
}))

vi.mock('@/store/hooks', () => ({
  useAppSelector: () => undefined,
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: string | Record<string, unknown>) =>
      typeof opts === 'string' ? opts : key,
  }),
}))

const noop = vi.fn()

function renderCard(item: DisplayClipboardItem) {
  render(
    <HistoryCard
      item={item}
      copySuccess={false}
      isDeleting={false}
      onCopy={noop}
      onDelete={noop}
      onToggleFavorite={noop}
      onClick={noop}
      onHoverChange={noop}
    />
  )
}

function renderInteractiveCard(item: DisplayClipboardItem, onCopy = vi.fn()) {
  const onCardClick = vi.fn()

  render(
    <HistoryCard
      item={item}
      copySuccess={false}
      isDeleting={false}
      onCopy={onCopy}
      onDelete={noop}
      onToggleFavorite={noop}
      onClick={onCardClick}
      onHoverChange={noop}
    />
  )
  fireEvent.mouseEnter(screen.getByTestId('history-card'))
  return { onCardClick, onCopy }
}

describe('HistoryCard', () => {
  it('owns hover feedback and releases the shortcut target when virtualized away', () => {
    const onHoverChange = vi.fn()
    const { unmount } = render(
      <HistoryCard
        item={{ id: 'hover-entry', type: 'text', activeTime: 1, content: null }}
        copySuccess={false}
        isDeleting={false}
        onCopy={noop}
        onDelete={noop}
        onToggleFavorite={noop}
        onClick={noop}
        onHoverChange={onHoverChange}
      />
    )
    const card = screen.getByTestId('history-card')
    const copyButton = screen.getByRole('button', { name: 'clipboard.item.actions.copy' })
    fireEvent.mouseEnter(card)
    expect(copyButton.parentElement).toHaveClass('opacity-100')
    expect(onHoverChange).toHaveBeenLastCalledWith('hover-entry', true)
    fireEvent.mouseLeave(card)
    expect(copyButton.parentElement).toHaveClass('opacity-0')
    expect(onHoverChange).toHaveBeenLastCalledWith('hover-entry', false)
    fireEvent.mouseEnter(card)
    unmount()
    expect(onHoverChange).toHaveBeenLastCalledWith('hover-entry', false)
  })

  it('shows code as a text card with a code tag', () => {
    renderCard({
      id: 'code-entry',
      type: 'text',
      content: { display_text: 'plain snippet', has_detail: false, size: 13 },
      activeTime: 1,
      contentTags: ['code'],
    } as DisplayClipboardItem)

    expect(screen.getByText('text')).toBeInTheDocument()
    expect(screen.getByText('code')).toBeInTheDocument()
  })

  it('keeps the rich text label when a content tag is present', () => {
    renderCard({
      id: 'richtext-entry',
      type: 'richtext',
      content: {
        display_text: 'https://example.com/article',
        has_detail: false,
        size: 28,
      },
      activeTime: 1,
      contentTags: ['link'],
    } as DisplayClipboardItem)

    expect(screen.getByText('richtext')).toBeInTheDocument()
    expect(screen.getByText('link')).toBeInTheDocument()
    expect(screen.queryByText('text')).not.toBeInTheDocument()
  })

  it('shows links as text cards with a link tag', () => {
    renderCard({
      id: 'link-entry',
      type: 'text',
      content: {
        display_text: 'https://example.com/docs',
        has_detail: false,
        size: 24,
      },
      activeTime: 1,
      contentTags: ['link'],
    } as DisplayClipboardItem)

    expect(screen.getByText('text')).toBeInTheDocument()
    expect(screen.getByText('link')).toBeInTheDocument()
  })

  it('renders links from structured URLs instead of preview text', () => {
    renderCard({
      id: 'link-entry',
      type: 'text',
      content: {
        display_text: 'https://example.com/preview-only',
        has_detail: false,
        size: 24,
        link_urls: ['https://example.com/docs'],
        link_domains: ['example.com'],
      },
      activeTime: 1,
      contentTags: ['link'],
    } as DisplayClipboardItem)

    expect(screen.getByText('example.com')).toBeInTheDocument()
    expect(screen.getByText('example.com/docs')).toBeInTheDocument()
    expect(screen.queryByText('example.com/preview-only')).not.toBeInTheDocument()
  })

  it('renders a file card with filename and formatted size', () => {
    renderCard({
      id: 'file-entry',
      type: 'file',
      content: {
        file_names: ['report.pdf'],
        file_sizes: [2048],
      },
      activeTime: 1,
    } as DisplayClipboardItem)

    expect(screen.getByText('file')).toBeInTheDocument()
    expect(screen.getByText('report.pdf')).toBeInTheDocument()
    expect(screen.getAllByText('2.00 KB').length).toBeGreaterThan(0)
  })

  it('joins code preview lines with a newline marker', () => {
    renderCard({
      id: 'code-preview-entry',
      type: 'text',
      content: {
        display_text: 'const value = 1\nreturn value',
        has_detail: false,
        size: 28,
      },
      contentTags: ['code'],
      activeTime: 1,
    } as DisplayClipboardItem)

    expect(screen.getByText('JavaScript')).toBeInTheDocument()
    expect(screen.getByText('↵').parentElement).toHaveTextContent('const value = 1 ↵ return value')
    expect(screen.queryByText('2')).not.toBeInTheDocument()
  })

  it('hides the hover actions after clicking an action button', async () => {
    const user = userEvent.setup()
    const { onCardClick, onCopy } = renderInteractiveCard({
      id: 'copy-entry',
      type: 'text',
      content: { display_text: 'copy me', char_count: 7 },
      activeTime: 1,
    } as DisplayClipboardItem)

    const copyButton = screen.getByRole('button', { name: 'clipboard.item.actions.copy' })

    expect(copyButton.parentElement).toHaveClass('opacity-100')

    await user.click(copyButton)

    expect(onCopy).toHaveBeenCalledWith('copy-entry')
    expect(onCardClick).not.toHaveBeenCalled()
    expect(copyButton).toHaveAttribute('tabindex', '-1')
    expect(copyButton.parentElement).toHaveClass('opacity-0')
  })
})
