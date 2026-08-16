import { render, screen } from '@testing-library/react'
import React from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { useHistoryController } from '@/hooks/useHistoryController'
import HistoryPage from '@/pages/HistoryPage'

type HistoryControllerState = ReturnType<typeof useHistoryController>

const controller = vi.hoisted(() => ({
  current: null as unknown,
}))

const windowFrame = vi.hoisted(() => ({
  searchInTitleBar: true,
}))

const titleBarSlot = vi.hoisted(() => ({
  host: null as HTMLElement | null,
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('framer-motion', async () => {
  const ReactModule = await import('react')
  return {
    m: {
      div: ({
        animate,
        children,
        initial,
        transition,
        ...props
      }: React.HTMLAttributes<HTMLDivElement> & {
        animate?: unknown
        initial?: unknown
        transition?: unknown
      }) =>
        ReactModule.createElement(
          'div',
          {
            ...props,
            'data-motion-animate': JSON.stringify(animate),
            'data-motion-initial': JSON.stringify(initial),
            'data-motion-transition': JSON.stringify(transition),
          },
          children
        ),
    },
  }
})

vi.mock('@/hooks/useWindowFrame', () => ({
  useWindowFrame: () => ({ searchInTitleBar: windowFrame.searchInTitleBar }),
}))

vi.mock('@/contexts/titlebar-slot-context', () => ({
  useTitleBarSlot: () => ({ rightSlotHost: titleBarSlot.host }),
}))

vi.mock('@/hooks/useHistoryController', () => ({
  useHistoryController: () => controller.current,
}))

vi.mock('@/components/history/composite-search', async () => {
  const ReactModule = await import('react')
  return {
    CompositeSearchBar: () => ReactModule.createElement('div', { 'data-testid': 'search-bar' }),
    HistoryFilterPanel: () =>
      ReactModule.createElement('aside', { 'data-testid': 'history-filter-panel' }),
  }
})

vi.mock('@/components/history/HistoryGrid', async () => {
  const ReactModule = await import('react')
  return {
    default: () => ReactModule.createElement('section', { 'data-testid': 'history-grid' }),
  }
})

vi.mock('@/components/clipboard/ClipboardPreview', async () => {
  const ReactModule = await import('react')
  return {
    default: ({ item }: { item: unknown | null }) =>
      ReactModule.createElement(
        'section',
        { 'data-testid': 'clipboard-preview' },
        item ? 'preview item' : 'preview empty'
      ),
  }
})

vi.mock('@/components/clipboard/ClipboardActionBar', async () => {
  const ReactModule = await import('react')
  return {
    default: () => ReactModule.createElement('div', { 'data-testid': 'clipboard-actions' }),
  }
})

vi.mock('@/components/clipboard/DeleteConfirmDialog', async () => {
  const ReactModule = await import('react')
  return {
    default: () => ReactModule.createElement('div', { 'data-testid': 'delete-dialog' }),
  }
})

vi.mock('@/components/ui/resizable', async () => {
  const ReactModule = await import('react')
  return {
    ResizableHandle: () => ReactModule.createElement('div', { 'data-testid': 'resize-handle' }),
    ResizablePanel: ({ children }: { children: React.ReactNode }) =>
      ReactModule.createElement('section', null, children),
    ResizablePanelGroup: ({ children }: { children: React.ReactNode }) =>
      ReactModule.createElement('div', { 'data-testid': 'resizable-group' }, children),
  }
})

function makeControllerState(
  overrides: Partial<HistoryControllerState> = {}
): HistoryControllerState {
  return {
    browseCount: 0,
    confirmDelete: vi.fn(),
    copySuccessId: null,
    deleteDialogOpen: false,
    deletingId: null,
    filter: {
      activeFilter: 'all',
      sourceFilter: null,
      submittedQuery: '',
      tagFilter: null,
      timeRange: null,
      extensionFilter: null,
    },
    filterActions: {
      setContentFilter: vi.fn(),
      setQuery: vi.fn(),
      setSourceFilter: vi.fn(),
      setTagFilter: vi.fn(),
      setTimeRange: vi.fn(),
      setExtensionFilter: vi.fn(),
      submitQuery: vi.fn(),
    },
    handleCardClick: vi.fn(),
    handleCopy: vi.fn(),
    handleLoadMore: vi.fn(),
    handleToggleFavorite: vi.fn(),
    hasMore: false,
    hoveredId: null,
    indexState: 'ready',
    isSearchActive: false,
    items: [],
    listRef: { current: null },
    requestDelete: vi.fn(),
    scrollState: null,
    searchInputRef: { current: null },
    searchLoading: false,
    searchableTags: [],
    seenIds: new Set<string>(),
    selectedId: null,
    selectedItem: null,
    setDeleteDialogOpen: vi.fn(),
    setHoveredId: vi.fn(),
    setScrollState: vi.fn(),
    sourceOptions: [],
    viewLabel: 'history.filter.all',
    ...overrides,
  } as HistoryControllerState
}

describe('HistoryPage', () => {
  beforeEach(() => {
    controller.current = makeControllerState()
    windowFrame.searchInTitleBar = true
    titleBarSlot.host = document.createElement('div')
    document.body.appendChild(titleBarSlot.host)
  })

  it('puts search in the title bar when the custom frame is active', () => {
    render(<HistoryPage />)

    expect(titleBarSlot.host).toContainElement(screen.getByTestId('search-bar'))
    expect(screen.queryByText('history.filter.all')).not.toBeInTheDocument()
  })

  it('keeps search in the page when the system frame is active', () => {
    windowFrame.searchInTitleBar = false

    render(<HistoryPage />)

    expect(titleBarSlot.host).toBeEmptyDOMElement()
    expect(screen.getByText('history.filter.all')).toBeInTheDocument()
    expect(screen.getByTestId('search-bar')).toBeInTheDocument()
  })

  it('animates the preview pane shortly after history rows start entering', () => {
    render(<HistoryPage />)

    const previewMotion = screen.getByTestId('history-preview-motion')

    expect(previewMotion).toHaveAttribute(
      'data-motion-initial',
      JSON.stringify({ opacity: 0, y: 16 })
    )
    expect(previewMotion).toHaveAttribute(
      'data-motion-animate',
      JSON.stringify({ opacity: 1, y: 0 })
    )
    expect(previewMotion).toHaveAttribute(
      'data-motion-transition',
      JSON.stringify({ type: 'spring', stiffness: 400, damping: 30, delay: 0.08 })
    )
    expect(screen.getByTestId('clipboard-preview')).toBeInTheDocument()
  })
})
