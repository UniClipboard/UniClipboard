import { render, screen } from '@testing-library/react'
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
})
