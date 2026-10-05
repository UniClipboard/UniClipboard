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

    await user.type(screen.getByRole('combobox'), '/image{Enter}')

    expect(props.onContentFilterChange).toHaveBeenCalledWith(Filter.Image)
    expect(props.onQuerySubmit).not.toHaveBeenCalled()
  })

  it('applies a time range from an on: token and treats time: as plain text', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar()

    const input = screen.getByRole('combobox')
    await user.type(input, 'on:today{Enter}')
    expect(props.onTimeRangeChange).toHaveBeenCalledWith('today')

    await user.type(input, 'time:today')
    expect(props.onQueryChange).toHaveBeenLastCalledWith('time:today')
  })

  it('picks a source device from an @ token', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar()

    await user.type(screen.getByRole('combobox'), '@mac{Enter}')

    expect(props.onSourceFilterChange).toHaveBeenCalledWith('device-1')
  })

  it('searches a path starting with / as text', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar()

    await user.type(screen.getByRole('combobox'), '/tmp/build.log{Enter}')

    expect(props.onQueryChange).toHaveBeenLastCalledWith('/tmp/build.log')
    expect(props.onQuerySubmit).toHaveBeenCalledWith('/tmp/build.log')
    expect(props.onContentFilterChange).not.toHaveBeenCalled()
  })

  it('adds a typed tag to the existing tag selection', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar({
      tagFilter: 'link',
      tagOptions: [
        { id: 'link', count: 3, isBuiltin: true },
        { id: 'code', count: 2, isBuiltin: true },
      ],
    })

    await user.type(screen.getByRole('combobox'), '#code ')

    expect(props.onTagFilterChange).toHaveBeenCalledWith('link,code')
  })

  it('keeps an already-selected tag when it is typed again', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar({ tagFilter: 'link,code' })

    await user.type(screen.getByRole('combobox'), '#code ')

    expect(props.onTagFilterChange).not.toHaveBeenCalled()
  })

  it('pops only the last tag back into the field on Backspace', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar({ tagFilter: 'link,code' })

    const input = screen.getByRole('combobox')
    await user.click(input)
    await user.keyboard('{Backspace}')

    expect(props.onTagFilterChange).toHaveBeenCalledWith('link')
    expect(input).toHaveValue('#code')
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

  it('shows per-candidate hit counts computed with the other filters held fixed', async () => {
    const user = userEvent.setup()
    const fetchCounts = vi.fn().mockResolvedValue([5, 0, 1234, 7])
    renderSearchBar({ timeRange: 'today', fetchCounts })

    await user.type(screen.getByRole('combobox'), '/')

    expect(await screen.findByText('1,234', {}, { timeout: 2000 })).toBeInTheDocument()
    expect(fetchCounts).toHaveBeenCalledTimes(1)
    const [queries] = fetchCounts.mock.calls[0]
    expect(queries).toEqual([
      { query: '', contentTypes: 'text', timePreset: 'today' },
      { query: '', contentTypes: 'html', timePreset: 'today' },
      { query: '', tags: 'image', timePreset: 'today' },
      { query: '', contentTypes: 'file', timePreset: 'today' },
    ])
  })

  it('qualifies a typed token by the other filters in the list variant', async () => {
    const user = userEvent.setup()
    // In-filter counts, then the candidate's total on its own.
    const fetchCounts = vi
      .fn()
      .mockImplementation(async (queries: { sourceDevices?: string }[]) =>
        queries.map(query => (query.sourceDevices ? 0 : 2))
      )
    renderSearchBar({ variant: 'list', sourceFilter: 'device-1', fetchCounts })

    // The chip names its dimension's syntax key; the list field has no ✕.
    expect(screen.getByText('from')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'history.composite.clearAll' })).toBeNull()

    await user.type(screen.getByRole('combobox'), '#c')
    expect(
      screen.getByText('history.composite.header.startingWith · from MacBook')
    ).toBeInTheDocument()
    expect(screen.getByText('#code')).toBeInTheDocument()
    expect(
      await screen.findByText(
        'history.subtitle · history.composite.noneInContext',
        {},
        { timeout: 2000 }
      )
    ).toBeInTheDocument()
  })

  it('ends a typed tag with "+ Create" and filters by the new tag once created', async () => {
    const user = userEvent.setup()
    const onCreateTag = vi.fn().mockResolvedValue('tag-new')
    const props = renderSearchBar({
      variant: 'list',
      tagOptions: [{ id: 'tag-release', count: 12, isBuiltin: false, name: 'release' }],
      onCreateTag,
    })

    await user.type(screen.getByRole('combobox'), '#re')
    const options = screen.getAllByRole('option')
    expect(options.map(option => option.textContent)).toEqual([
      expect.stringContaining('#release'),
      expect.stringContaining('history.tags.createNamed'),
    ])

    await user.click(options[1])
    expect(onCreateTag).toHaveBeenCalledWith('re')
    await vi.waitFor(() => expect(props.onTagFilterChange).toHaveBeenCalledWith('tag-new'))
  })

  it('offers no "+ Create" for a tag that already exists', async () => {
    const user = userEvent.setup()
    renderSearchBar({
      variant: 'list',
      tagOptions: [{ id: 'tag-release', count: 12, isBuiltin: false, name: 'release' }],
      onCreateTag: vi.fn(),
    })

    await user.type(screen.getByRole('combobox'), '#Release')
    expect(screen.queryByText('history.tags.createNamed')).toBeNull()
  })

  it('offers no "+ Create" for a builtin tag typed by its id', async () => {
    const user = userEvent.setup()
    renderSearchBar({
      variant: 'list',
      tagOptions: [{ id: 'code', count: 2, isBuiltin: true }],
      onCreateTag: vi.fn(),
    })

    await user.type(screen.getByRole('combobox'), '#code')
    expect(screen.queryByText('history.tags.createNamed')).toBeNull()
  })

  it('searches a #word naming no tag as text, with "+ Create" one arrow away', async () => {
    const user = userEvent.setup()
    const onCreateTag = vi.fn().mockResolvedValue('tag-new')
    const props = renderSearchBar({ variant: 'list', tagOptions: [], onCreateTag })

    const input = screen.getByRole('combobox')
    await user.type(input, '#hotfix{Enter}')
    expect(props.onQuerySubmit).toHaveBeenCalledWith('#hotfix')
    expect(onCreateTag).not.toHaveBeenCalled()

    await user.click(input)
    await user.keyboard('{ArrowDown}{Enter}')
    expect(onCreateTag).toHaveBeenCalledWith('hotfix')
  })

  it('reopens the last chip as an editable token on Backspace in an empty input', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar({ contentFilter: Filter.Image, extensionFilter: 'md' })

    const input = screen.getByRole('combobox')
    await user.click(input)
    await user.keyboard('{Backspace}')

    expect(props.onExtensionFilterChange).toHaveBeenCalledWith(null)
    expect(props.onContentFilterChange).not.toHaveBeenCalled()
    expect(input).toHaveValue('ext:md')
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

  it('toggles tags in and out of a multi-tag selection', async () => {
    const user = userEvent.setup()
    const props = renderFilterPanel({
      contentFilter: Filter.All,
      tagFilter: 'link,code',
      tagOptions: [
        { id: 'link', count: 3, isBuiltin: true },
        { id: 'code', count: 2, isBuiltin: true },
        { id: 'image', count: 1, isBuiltin: true },
      ],
    })

    await user.click(screen.getByRole('button', { name: 'code', pressed: true }))
    await user.click(screen.getByRole('button', { name: 'image', pressed: false }))

    expect(props.onTagFilterChange).toHaveBeenNthCalledWith(1, 'link')
    expect(props.onTagFilterChange).toHaveBeenNthCalledWith(2, 'link,code,image')
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

    expect(selectedRow.querySelector('span[aria-hidden="true"]')).toHaveClass('bg-muted/50')
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
