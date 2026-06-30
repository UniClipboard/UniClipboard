import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import HistoryGrid from '@/components/history/HistoryGrid'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/components/history/HistoryCard', async () => {
  const React = await import('react')
  return {
    default: ({ item }: { item: DisplayClipboardItem }) =>
      React.createElement('div', { 'data-testid': `history-row-${item.id}` }, item.id),
  }
})

vi.mock('@/components/history/HistoryGridRow', async () => {
  const React = await import('react')
  return {
    default: ({ item }: { item: DisplayClipboardItem }) =>
      React.createElement('div', { 'data-testid': `history-row-${item.id}` }, item.id),
  }
})

vi.mock('react-virtuoso', async () => {
  const React = await import('react')
  return {
    Virtuoso: ({
      data,
      endReached,
      itemContent,
    }: {
      data: DisplayClipboardItem[]
      endReached?: () => void
      itemContent: (index: number, item: DisplayClipboardItem) => React.ReactNode
    }) =>
      React.createElement(
        'div',
        { 'data-testid': 'history-virtuoso' },
        data
          .slice(0, 3)
          .map((item, index) =>
            React.createElement(
              'div',
              { key: item.id, 'data-testid': `virtuoso-slot-${item.id}` },
              itemContent(index, item)
            )
          ),
        React.createElement(
          'button',
          {
            type: 'button',
            onClick: () => endReached?.(),
          },
          'reach end'
        )
      ),
  }
})

const noop = vi.fn()

function makeItem(index: number): DisplayClipboardItem {
  return {
    id: `entry-${index}`,
    type: 'text',
    content: {
      display_text: `Clipboard entry ${index}`,
      has_detail: false,
      size: index,
    },
    activeTime: index,
  }
}

function renderGrid({
  items = [makeItem(0)],
  hasMore = false,
  searchLoading = false,
  isSearchActive = false,
  submittedQuery = '',
  onLoadMore = vi.fn(),
}: {
  items?: DisplayClipboardItem[]
  hasMore?: boolean
  searchLoading?: boolean
  isSearchActive?: boolean
  submittedQuery?: string
  onLoadMore?: () => void
} = {}) {
  render(
    <HistoryGrid
      items={items}
      seenIds={new Set<string>()}
      selectedId={null}
      isSearchActive={isSearchActive}
      submittedQuery={submittedQuery}
      searchLoading={searchLoading}
      hoveredId={null}
      copySuccessId={null}
      deletingId={null}
      hasMore={hasMore}
      onLoadMore={onLoadMore}
      onCopy={noop}
      onDelete={noop}
      onToggleFavorite={noop}
      onCardClick={noop}
      onHoverChange={noop}
    />
  )

  return { onLoadMore }
}

describe('HistoryGrid', () => {
  it('renders only the visible subset through the virtualized list', () => {
    const items = Array.from({ length: 25 }, (_value, index) => makeItem(index))

    renderGrid({ items })

    expect(screen.getByTestId('history-virtuoso')).toBeInTheDocument()
    expect(screen.getByTestId('history-row-entry-0')).toBeInTheDocument()
    expect(screen.getByTestId('history-row-entry-2')).toBeInTheDocument()
    expect(screen.queryByTestId('history-row-entry-24')).not.toBeInTheDocument()
  })

  it('loads more when the virtualized list reaches the end and the page can grow', async () => {
    const user = userEvent.setup()
    const onLoadMore = vi.fn()

    renderGrid({
      items: Array.from({ length: 25 }, (_value, index) => makeItem(index)),
      hasMore: true,
      searchLoading: false,
      onLoadMore,
    })

    await user.click(screen.getByRole('button', { name: 'reach end' }))

    expect(onLoadMore).toHaveBeenCalledTimes(1)
  })

  it('does not load more while a search page is already loading', async () => {
    const user = userEvent.setup()
    const onLoadMore = vi.fn()

    renderGrid({
      items: Array.from({ length: 25 }, (_value, index) => makeItem(index)),
      hasMore: true,
      searchLoading: true,
      onLoadMore,
    })

    await user.click(screen.getByRole('button', { name: 'reach end' }))

    expect(onLoadMore).not.toHaveBeenCalled()
  })

  it('keeps the loading and empty states unchanged', () => {
    const { rerender } = render(
      <HistoryGrid
        items={[]}
        seenIds={new Set<string>()}
        selectedId={null}
        isSearchActive={false}
        submittedQuery=""
        searchLoading={true}
        hoveredId={null}
        copySuccessId={null}
        deletingId={null}
        hasMore={false}
        onLoadMore={noop}
        onCopy={noop}
        onDelete={noop}
        onToggleFavorite={noop}
        onCardClick={noop}
        onHoverChange={noop}
      />
    )

    expect(screen.getByText('clipboard.search.searching')).toBeInTheDocument()

    rerender(
      <HistoryGrid
        items={[]}
        seenIds={new Set<string>()}
        selectedId={null}
        isSearchActive={false}
        submittedQuery=""
        searchLoading={false}
        hoveredId={null}
        copySuccessId={null}
        deletingId={null}
        hasMore={false}
        onLoadMore={noop}
        onCopy={noop}
        onDelete={noop}
        onToggleFavorite={noop}
        onCardClick={noop}
        onHoverChange={noop}
      />
    )

    expect(screen.getByText('clipboard.content.noClipboardItems')).toBeInTheDocument()
  })
})
