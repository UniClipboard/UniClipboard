import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { useState } from 'react'
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
      isHovered={false}
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

  function InteractiveCard() {
    const [hoveredId, setHoveredId] = useState<string | null>(item.id)

    return (
      <HistoryCard
        item={item}
        isHovered={hoveredId === item.id}
        copySuccess={false}
        isDeleting={false}
        onCopy={onCopy}
        onDelete={noop}
        onToggleFavorite={noop}
        onClick={onCardClick}
        onHoverChange={setHoveredId}
      />
    )
  }

  render(<InteractiveCard />)
  return { onCardClick, onCopy }
}

describe('HistoryCard', () => {
  it('shows code as a text card with a code tag', () => {
    renderCard({
      id: 'code-entry',
      type: 'code',
      content: { code: 'plain snippet' },
      activeTime: 1,
      contentTags: ['code'],
    } as DisplayClipboardItem)

    expect(screen.getByText('text')).toBeInTheDocument()
    expect(screen.getByText('code')).toBeInTheDocument()
  })

  it('shows links as text cards with a link tag', () => {
    renderCard({
      id: 'link-entry',
      type: 'link',
      content: {
        urls: ['https://example.com/docs'],
        domains: ['example.com'],
      },
      activeTime: 1,
      contentTags: ['link'],
    } as DisplayClipboardItem)

    expect(screen.getByText('text')).toBeInTheDocument()
    expect(screen.getByText('link')).toBeInTheDocument()
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

  it('renders code preview lines with line numbers', () => {
    renderCard({
      id: 'code-preview-entry',
      type: 'code',
      content: {
        code: 'const value = 1\nreturn value',
      },
      activeTime: 1,
    } as DisplayClipboardItem)

    expect(screen.getByText('JavaScript')).toBeInTheDocument()
    expect(screen.getByText('const')).toBeInTheDocument()
    expect(screen.getByText('return')).toBeInTheDocument()
    expect(screen.getAllByText('1').length).toBeGreaterThanOrEqual(2)
    expect(screen.getByText('2')).toBeInTheDocument()
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
