import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import CompositeSearchBar from '../CompositeSearchBar'
import HistoryFilterPanel from '../HistoryFilterPanel'

vi.mock('@/hooks/useShortcut', () => ({
  useShortcut: vi.fn(),
}))

vi.mock('@/contexts/sidebar-slot-context', () => ({
  useSidebarSlot: () => ({
    contentToolbarHost: null,
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: string | Record<string, unknown>) =>
      typeof opts === 'string'
        ? opts
        : typeof opts?.defaultValue === 'string'
          ? opts.defaultValue
          : key,
  }),
}))

function renderSearchBar(overrides: Partial<React.ComponentProps<typeof CompositeSearchBar>> = {}) {
  const props: React.ComponentProps<typeof CompositeSearchBar> = {
    contentFilter: Filter.All,
    sourceFilter: null,
    tagFilter: null,
    timeRange: 'all_time' as TimeRangePreset,
    extensionFilter: null,
    onContentFilterChange: vi.fn(),
    onTagFilterChange: vi.fn(),
    onSourceFilterChange: vi.fn(),
    onTimeRangeChange: vi.fn(),
    onExtensionFilterChange: vi.fn(),
    onQueryChange: vi.fn(),
    onQuerySubmit: vi.fn(),
    sourceOptions: [{ id: 'device-1', name: 'MacBook', kind: 'p2p' }],
    tagOptions: [{ id: 'code', count: 2, isBuiltin: true }],
    totalCount: 12,
    inputRef: { current: null },
    ...overrides,
  }

  render(<CompositeSearchBar {...props} />)
  return props
}

describe('CompositeSearchBar', () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn()
  })

  it('submits free text with Enter when no suggestion is highlighted', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar()

    const input = screen.getByRole('combobox', { name: 'history.composite.title' })
    await user.type(input, 'release notes{Enter}')

    expect(props.onQueryChange).toHaveBeenLastCalledWith('release notes')
    expect(props.onQuerySubmit).toHaveBeenCalledWith('release notes')
  })

  it('disables browser text correction and completion', () => {
    renderSearchBar()

    const input = screen.getByRole('combobox', { name: 'history.composite.title' })
    expect(input).toHaveAttribute('autocorrect', 'off')
    expect(input).toHaveAttribute('autocapitalize', 'off')
    expect(input).toHaveAttribute('autocomplete', 'off')
    expect(input).toHaveAttribute('spellcheck', 'false')
  })

  it('applies a typed content filter token instead of submitting it as text', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar()

    await user.type(screen.getByRole('combobox'), 'type:image{Enter}')

    expect(props.onContentFilterChange).toHaveBeenCalledWith(Filter.Image)
    expect(props.onQuerySubmit).not.toHaveBeenCalled()
  })

  it('clears all active dimensions from the clear button', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar({
      contentFilter: Filter.File,
      sourceFilter: 'device-1',
      tagFilter: 'code',
      timeRange: 'today',
      extensionFilter: 'md',
    })

    await user.click(screen.getByRole('button', { name: 'history.composite.clearAll' }))

    expect(props.onContentFilterChange).toHaveBeenCalledWith(Filter.All)
    expect(props.onTagFilterChange).toHaveBeenCalledWith(null)
    expect(props.onSourceFilterChange).toHaveBeenCalledWith(null)
    expect(props.onTimeRangeChange).toHaveBeenCalledWith('all_time')
    expect(props.onExtensionFilterChange).toHaveBeenCalledWith(null)
    expect(props.onQueryChange).toHaveBeenCalledWith('')
  })
})

function renderFilterPanel(
  overrides: Partial<React.ComponentProps<typeof HistoryFilterPanel>> = {}
) {
  const props: React.ComponentProps<typeof HistoryFilterPanel> = {
    contentFilter: Filter.Favorited,
    sourceFilter: null,
    tagFilter: null,
    timeRange: 'all_time' as TimeRangePreset,
    extensionFilter: null,
    onContentFilterChange: vi.fn(),
    onTagFilterChange: vi.fn(),
    onSourceFilterChange: vi.fn(),
    onTimeRangeChange: vi.fn(),
    onExtensionFilterChange: vi.fn(),
    sourceOptions: [],
    tagOptions: [],
    ...overrides,
  }

  render(<HistoryFilterPanel {...props} />)
  return props
}

describe('HistoryFilterPanel', () => {
  it('replaces the all icon with a clear action while any filter is active', async () => {
    const user = userEvent.setup()
    const props = renderFilterPanel({
      sourceFilter: 'peer-1',
      tagFilter: 'code',
      timeRange: 'today',
      extensionFilter: 'txt',
    })

    const clearButton = screen.getByRole('button', { name: 'history.composite.clearAll' })
    expect(clearButton.querySelector('svg')).toHaveClass('lucide-x')

    await user.click(clearButton)

    expect(props.onContentFilterChange).toHaveBeenCalledWith(Filter.All)
    expect(props.onTagFilterChange).toHaveBeenCalledWith(null)
    expect(props.onSourceFilterChange).toHaveBeenCalledWith(null)
    expect(props.onTimeRangeChange).toHaveBeenCalledWith('all_time')
    expect(props.onExtensionFilterChange).toHaveBeenCalledWith(null)
  })

  it('keeps the all icon when no filter is active', () => {
    renderFilterPanel({ contentFilter: Filter.All })

    const allButton = screen.getByRole('button', { name: 'history.filter.all', pressed: true })
    expect(allButton.querySelector('svg')).toHaveClass('lucide-layout-grid')
    expect(
      screen.queryByRole('button', { name: 'history.composite.clearAll' })
    ).not.toBeInTheDocument()
  })

  it('uses a restrained selected-row treatment', () => {
    renderFilterPanel()

    const selectedRow = screen.getByRole('button', {
      name: 'history.filter.favorited',
      pressed: true,
    })
    const selectedIcon = selectedRow.querySelector('svg')

    expect(selectedRow.className).toContain('bg-muted/50')
    expect(selectedRow.className).toContain('text-foreground')
    expect(selectedRow.className).not.toContain('bg-primary')
    expect(selectedRow.className).not.toContain('shadow')
    expect(selectedRow.className).not.toContain('ring-')
    expect(selectedRow.className).not.toContain('font-medium')
    expect(selectedIcon).toHaveClass('opacity-80')
    expect(selectedRow).not.toHaveTextContent('history.filter.favorited')
  })

  it('uses a fixed-width horizontal strip and maps the wheel to horizontal scrolling', () => {
    renderFilterPanel({
      tagOptions: [
        { id: 'code', count: 2, isBuiltin: true },
        { id: 'link', count: 1, isBuiltin: true },
      ],
    })

    const strip = screen.getByTestId('history-filter-strip')
    expect(strip).toHaveClass('w-fit', 'max-w-72', 'overflow-x-auto', 'rounded-full')

    fireEvent.wheel(strip, { deltaY: 40 })
    expect(strip.scrollLeft).toBe(40)
  })
})
