import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import CompositeSearchBar from '../CompositeSearchBar'

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
    onContentFilterChange: vi.fn(),
    onTagFilterChange: vi.fn(),
    onSourceFilterChange: vi.fn(),
    onTimeRangeChange: vi.fn(),
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
  it('submits free text with Enter when no suggestion is highlighted', async () => {
    const user = userEvent.setup()
    const props = renderSearchBar()

    const input = screen.getByRole('combobox', { name: 'history.composite.title' })
    await user.type(input, 'release notes{Enter}')

    expect(props.onQueryChange).toHaveBeenLastCalledWith('release notes')
    expect(props.onQuerySubmit).toHaveBeenCalledWith('release notes')
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
    })

    await user.click(screen.getByRole('button', { name: 'history.composite.clearAll' }))

    expect(props.onContentFilterChange).toHaveBeenCalledWith(Filter.All)
    expect(props.onTagFilterChange).toHaveBeenCalledWith(null)
    expect(props.onSourceFilterChange).toHaveBeenCalledWith(null)
    expect(props.onTimeRangeChange).toHaveBeenCalledWith('all_time')
    expect(props.onQueryChange).toHaveBeenCalledWith('')
  })
})
