import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import QuickPanelTagFilterBar from '../QuickPanelTagFilterBar'
import type { SearchTagOption } from '@/lib/search-tags'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      ({
        'history.composite.dimension.tag': 'Tags',
        'history.type.code': 'Code',
      })[key] ?? (typeof options?.defaultValue === 'string' ? options.defaultValue : key),
  }),
}))

const tagOptions: SearchTagOption[] = [
  { id: 'code', count: 3, isBuiltin: true },
  { id: 'project', count: 1, isBuiltin: false },
]

describe('QuickPanelTagFilterBar', () => {
  it('shows every provided tag in a horizontally scrollable row', () => {
    const { container } = render(
      <QuickPanelTagFilterBar tagFilter={null} tagOptions={tagOptions} onChange={vi.fn()} />
    )

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

    rerender(<QuickPanelTagFilterBar tagFilter="code" tagOptions={tagOptions} onChange={onChange} />)
    await user.click(screen.getByRole('button', { name: 'Code' }))

    expect(onChange).toHaveBeenLastCalledWith(null)
  })
})
