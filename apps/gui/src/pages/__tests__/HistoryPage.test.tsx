import { act, render, screen, waitFor } from '@testing-library/react'
import React from 'react'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { useHistoryController } from '@/hooks/useHistoryController'
import HistoryPage from '@/pages/HistoryPage'

type HistoryControllerState = ReturnType<typeof useHistoryController>

const controller = vi.hoisted(() => ({
  current: null as unknown,
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
  const motionElement =
    (tag: 'button' | 'div' | 'section') =>
    ({
      animate,
      children,
      exit: _exit,
      initial,
      layoutId,
      transition,
      ...props
    }: React.HTMLAttributes<HTMLElement> & {
      animate?: unknown
      exit?: unknown
      initial?: unknown
      layoutId?: string
      transition?: unknown
    }) =>
      ReactModule.createElement(
        tag,
        {
          ...props,
          'data-motion-animate': JSON.stringify(animate),
          'data-motion-initial': JSON.stringify(initial),
          'data-motion-layout-id': layoutId,
          'data-motion-transition': JSON.stringify(transition),
        },
        children
      )
  return {
    AnimatePresence: ({ children }: { children: React.ReactNode }) => children,
    LayoutGroup: ({ children }: { children: React.ReactNode }) => children,
    m: {
      button: motionElement('button'),
      div: motionElement('div'),
      section: motionElement('section'),
    },
  }
})

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

vi.mock('@/components/history/composite-search', async importOriginal => {
  const ReactModule = await import('react')
  const actual = await importOriginal<typeof import('@/components/history/composite-search')>()
  return {
    ...actual,
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

vi.mock('@/components/history/sidebar/HistorySidebar', async () => {
  const ReactModule = await import('react')
  return {
    default: () => ReactModule.createElement('nav', { 'data-testid': 'history-sidebar' }),
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
      timeRange: 'all_time',
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

function renderPage() {
  return render(
    <MemoryRouter initialEntries={['/history']}>
      <HistoryPage />
    </MemoryRouter>
  )
}

describe('HistoryPage', () => {
  beforeEach(() => {
    shortcuts.configs = []
    controller.current = makeControllerState()
  })

  it('focuses the list-column search from the configurable shortcut', async () => {
    renderPage()

    const shortcut = shortcuts.configs.find(config => config.id === 'clipboard.search')
    expect(shortcut?.key).toBe('mod+f')

    act(() => shortcut?.handler())
    const input = screen.getByRole('combobox', { name: 'history.composite.title' })
    await waitFor(() => expect(input).toHaveFocus())
  })

  it('opens search with slash only outside form fields', () => {
    renderPage()

    const shortcut = shortcuts.configs.find(
      config => Array.isArray(config.key) && config.key.includes('/') && config.key.includes('、')
    )
    expect(shortcut).toBeDefined()
    expect(shortcut?.id).toBeUndefined()
    expect(shortcut?.enableOnFormTags).not.toBe(true)
    expect(shortcut?.useKey).toBe(true)
  })

  it('animates the preview pane shortly after history rows start entering', () => {
    renderPage()

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
    renderPage()

    expect(screen.getByTestId('history-list-panel')).toHaveAttribute('data-default-size', '42%')
    expect(screen.getByTestId('history-list-panel')).toHaveAttribute('data-min-size', '20rem')
    expect(screen.getByTestId('history-list-panel')).toHaveAttribute('data-max-size', '36rem')
    expect(screen.getByTestId('history-preview-panel')).toHaveAttribute('data-default-size', '58%')
  })
})
