import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import CompositeSearchBar from '../CompositeSearchBar'

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
  const onUnhandledKeyDown = vi.fn()
  const onQuerySubmit = vi.fn()
  render(
    <CompositeSearchBar
      contentFilter={Filter.All}
      sourceFilter={null}
      tagFilter={null}
      timeRange={'all_time' as TimeRangePreset}
      extensionFilter={null}
      onContentFilterChange={vi.fn()}
      onTagFilterChange={vi.fn()}
      onSourceFilterChange={vi.fn()}
      onTimeRangeChange={vi.fn()}
      onExtensionFilterChange={vi.fn()}
      onQueryChange={vi.fn()}
      onQuerySubmit={onQuerySubmit}
      sourceOptions={[]}
      tagOptions={[]}
      totalCount={0}
      inputRef={{ current: null }}
      clearShortcutEnabled={false}
      suggestionActivation="intentional"
      showFilterPanelButton
      onUnhandledKeyDown={onUnhandledKeyDown}
      {...overrides}
    />
  )
  return { onUnhandledKeyDown, onQuerySubmit }
}

describe('CompositeSearchBar quick window usage', () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn()
  })

  it('renders without the main-window shortcut provider when shortcut registration is disabled', () => {
    renderSearchBar()

    expect(screen.getByRole('combobox', { name: 'history.composite.title' })).toBeInTheDocument()
  })

  it('does not open suggestions when the auto-focused input receives focus', async () => {
    const user = userEvent.setup()
    renderSearchBar()

    await user.click(screen.getByRole('combobox', { name: 'history.composite.title' }))

    expect(screen.getByRole('combobox', { name: 'history.composite.title' })).toHaveAttribute(
      'aria-expanded',
      'false'
    )
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument()
  })

  it('opens the composite filter panel from the leading button for pointer users', async () => {
    const user = userEvent.setup()
    renderSearchBar()

    const button = screen.getByRole('button', { name: 'history.composite.openFilters' })
    const input = screen.getByRole('combobox', { name: 'history.composite.title' })

    expect(button.parentElement).not.toBe(input.parentElement)

    await user.click(button)

    expect(input).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByRole('listbox', { name: 'history.composite.title' })).toBeInTheDocument()
    expect(input).toHaveFocus()
  })

  it('keeps record navigation keys available while typing a plain search query', async () => {
    const user = userEvent.setup()
    const { onUnhandledKeyDown } = renderSearchBar()

    await user.type(screen.getByRole('combobox', { name: 'history.composite.title' }), 'invoice')
    await user.keyboard('{ArrowDown}')

    expect(screen.getByRole('combobox', { name: 'history.composite.title' })).toHaveAttribute(
      'aria-expanded',
      'false'
    )
    expect(onUnhandledKeyDown).toHaveBeenCalledWith(expect.objectContaining({ key: 'ArrowDown' }))
  })

  it('delegates Enter to the selected record while typing a plain search query', async () => {
    const user = userEvent.setup()
    const { onUnhandledKeyDown, onQuerySubmit } = renderSearchBar()

    await user.type(screen.getByRole('combobox', { name: 'history.composite.title' }), 'invoice')
    await user.keyboard('{Enter}')

    expect(onQuerySubmit).not.toHaveBeenCalled()
    expect(onUnhandledKeyDown).toHaveBeenCalledWith(expect.objectContaining({ key: 'Enter' }))
  })

  it('opens filter candidates from an explicit token and returns to the record list after selection', async () => {
    const user = userEvent.setup()
    const onContentFilterChange = vi.fn()
    renderSearchBar({ onContentFilterChange })

    const input = screen.getByRole('combobox', { name: 'history.composite.title' })
    await user.type(input, 'type:')

    expect(input).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByRole('listbox', { name: 'history.composite.title' })).toBeInTheDocument()

    await user.keyboard('{ArrowDown}{Enter}')

    expect(onContentFilterChange).toHaveBeenCalledWith(Filter.Text)
    expect(input).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument()
  })
})
