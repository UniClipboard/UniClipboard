import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { useHistoryController } from '@/hooks/useHistoryController'
import HistoryPage from '@/pages/HistoryPage'

type HistoryControllerState = ReturnType<typeof useHistoryController>

const controller = vi.hoisted(() => ({
  current: null as unknown,
}))

const sidebarSlot = vi.hoisted(() => ({
  contentToolbarHost: null as HTMLElement | null,
}))

const shortcuts = vi.hoisted(() => ({
  configs: [] as Array<{
    id?: string
    key: string | string[]
    handler: () => void
    enableOnFormTags?: boolean
    useKey?: boolean
  }>,
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

vi.mock('@/contexts/sidebar-slot-context', () => ({
  useSidebarSlot: () => ({
    contentToolbarHost: sidebarSlot.contentToolbarHost,
  }),
}))

vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: (config: {
    id?: string
    key: string | string[]
    handler: () => void
    enableOnFormTags?: boolean
    useKey?: boolean
  }) => {
    shortcuts.configs.push(config)
  },
}))

vi.mock('@/hooks/useHistoryController', () => ({
  useHistoryController: () => controller.current,
}))

vi.mock('@/components/history/composite-search', async () => {
  const ReactModule = await import('react')
  return {
    HistoryFilterPanel: () =>
      ReactModule.createElement('aside', { 'data-testid': 'history-filter-panel' }),
    HistorySearchPanel: () =>
      ReactModule.createElement('aside', { 'data-testid': 'history-search-panel' }),
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
    ResizablePanel: ({
      children,
      id,
      defaultSize,
      minSize,
      maxSize,
    }: {
      children: React.ReactNode
      id?: string
      defaultSize?: string
      minSize?: string
      maxSize?: string
    }) =>
      ReactModule.createElement(
        'section',
        {
          'data-testid': id ? `${id}-panel` : undefined,
          'data-default-size': defaultSize,
          'data-min-size': minSize,
          'data-max-size': maxSize,
        },
        children
      ),
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
    shortcuts.configs = []
    controller.current = makeControllerState()
    sidebarSlot.contentToolbarHost = document.createElement('div')
    document.body.append(sidebarSlot.contentToolbarHost)
  })

  it('puts the horizontal filter strip in the content toolbar', () => {
    render(<HistoryPage />)

    expect(sidebarSlot.contentToolbarHost).toContainElement(
      screen.getByTestId('history-filter-panel')
    )
  })

  it('puts the search control in the content toolbar', () => {
    render(<HistoryPage />)

    expect(sidebarSlot.contentToolbarHost).toContainElement(
      screen.getByRole('button', { name: 'history.composite.title' })
    )
  })

  it('opens and focuses search from the configurable shortcut', async () => {
    render(<HistoryPage />)

    const shortcut = shortcuts.configs.find(config => config.id === 'clipboard.search')
    expect(shortcut?.key).toBe('mod+f')

    act(() => shortcut?.handler())
    const input = await screen.findByRole('textbox', { name: 'history.searchPlaceholder' })
    await waitFor(() => expect(input).toHaveFocus())
  })

  it('opens search with slash only outside form fields', () => {
    render(<HistoryPage />)

    const shortcut = shortcuts.configs.find(
      config => Array.isArray(config.key) && config.key.includes('/') && config.key.includes('、')
    )
    expect(shortcut).toBeDefined()
    expect(shortcut?.id).toBeUndefined()
    expect(shortcut?.enableOnFormTags).not.toBe(true)
    expect(shortcut?.useKey).toBe(true)
  })

  it('disables browser text correction in the toolbar search', async () => {
    const user = userEvent.setup()
    render(<HistoryPage />)

    await user.click(screen.getByRole('button', { name: 'history.composite.title' }))
    const input = screen.getByRole('textbox', { name: 'history.searchPlaceholder' })

    expect(input).toHaveAttribute('autocorrect', 'off')
    expect(input).toHaveAttribute('autocapitalize', 'off')
    expect(input).toHaveAttribute('autocomplete', 'off')
    expect(input).toHaveAttribute('spellcheck', 'false')
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

  it('keeps the history list within readable bounds while preview uses extra width', () => {
    render(<HistoryPage />)

    expect(screen.getByTestId('history-list-panel')).toHaveAttribute('data-default-size', '42%')
    expect(screen.getByTestId('history-list-panel')).toHaveAttribute('data-min-size', '20rem')
    expect(screen.getByTestId('history-list-panel')).toHaveAttribute('data-max-size', '36rem')
    expect(screen.getByTestId('history-preview-panel')).toHaveAttribute('data-default-size', '58%')
  })
})
