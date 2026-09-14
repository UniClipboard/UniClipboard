import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { SearchTagOption } from '@/lib/search-tags'
import QuickPanelTagFilterBar from '../QuickPanelTagFilterBar'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    i18n: { resolvedLanguage: 'en' },
    t: (key: string, options?: Record<string, unknown>) =>
      ({
        'history.composite.dimension.tag': 'Tags',
        'history.type.code': 'Code',
        'clipboard.item.expand': 'Expand',
        'clipboard.item.collapse': 'Collapse',
      })[key] ?? (typeof options?.defaultValue === 'string' ? options.defaultValue : key),
  }),
}))

const tagOptions: SearchTagOption[] = [
  { id: 'code', count: 3, isBuiltin: true },
  { id: 'project', count: 1, isBuiltin: false },
]

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('QuickPanelTagFilterBar', () => {
  it('shows every provided tag in a horizontally scrollable row', () => {
    const { container } = render(
      <QuickPanelTagFilterBar tagFilter={null} tagOptions={tagOptions} onChange={vi.fn()} />
    )

    expect(screen.queryByText('Tags')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Expand' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Code' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'project' })).toBeInTheDocument()
    expect(container.querySelector('[data-testid="quick-panel-tag-filter-list"]')).toHaveClass(
      'overflow-x-auto'
    )
  })

  it('applies an unselected tag and clears the selected tag on a second click', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    const { rerender } = render(
      <QuickPanelTagFilterBar tagFilter={null} tagOptions={tagOptions} onChange={onChange} />
    )

    await user.click(screen.getByRole('button', { name: 'Code' }))
    expect(onChange).toHaveBeenCalledWith('code')

    rerender(
      <QuickPanelTagFilterBar tagFilter="code" tagOptions={tagOptions} onChange={onChange} />
    )
    await user.click(screen.getByRole('button', { name: 'Code' }))

    expect(onChange).toHaveBeenLastCalledWith(null)
  })
  it('expands overflowing tags, preserves multiple selections, and rechecks on resize', async () => {
    let availableWidth = 100
    let resize = () => {}
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => availableWidth)
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
      function (this: HTMLElement) {
        return { width: this.hasAttribute('aria-pressed') ? 80 : 0 } as DOMRect
      }
    )
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(callback: () => void) {
          resize = callback
        }
        observe() {}
        disconnect() {}
      }
    )
    const user = userEvent.setup()
    const onChange = vi.fn()
    const { rerender } = render(
      <QuickPanelTagFilterBar tagFilter="code" tagOptions={tagOptions} onChange={onChange} />
    )
    await user.click(screen.getByRole('button', { name: 'Expand' }))
    expect(screen.getByRole('button', { name: 'Collapse' })).toHaveAttribute(
      'aria-expanded',
      'true'
    )
    expect(screen.getByTestId('quick-panel-tag-filter-list')).toHaveClass(
      'flex-wrap',
      'overflow-y-auto'
    )
    await user.click(screen.getByRole('button', { name: 'project' }))
    expect(onChange).toHaveBeenLastCalledWith('code,project')
    rerender(
      <QuickPanelTagFilterBar
        tagFilter="code,project"
        tagOptions={tagOptions}
        onChange={onChange}
      />
    )
    expect(screen.getByRole('button', { name: 'Code' })).toHaveAttribute('aria-pressed', 'true')
    expect(screen.getByRole('button', { name: 'project' })).toHaveAttribute('aria-pressed', 'true')
    await user.click(screen.getByRole('button', { name: 'Collapse' }))
    expect(onChange).toHaveBeenCalledTimes(1)
    await user.click(screen.getByRole('button', { name: 'Code' }))
    expect(onChange).toHaveBeenLastCalledWith('project')
    act(() => {
      availableWidth = 300
      resize()
    })
    expect(screen.queryByRole('button', { name: 'Expand' })).not.toBeInTheDocument()
  })
})
