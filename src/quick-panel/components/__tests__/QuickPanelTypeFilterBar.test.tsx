import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import QuickPanelTypeFilterBar from '../QuickPanelTypeFilterBar'
import { Filter } from '@/api/clipboardItems'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        'history.filter.all': 'All',
        'history.type.text': 'Text',
        'history.type.richtext': 'Rich Text',
        'history.type.image': 'Image',
        'history.type.file': 'File',
      })[key] ?? key,
  }),
}))

describe('QuickPanelTypeFilterBar', () => {
  it('keeps every content type visible and applies the selected type', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()

    render(<QuickPanelTypeFilterBar activeFilter={Filter.All} onChange={onChange} />)

    expect(screen.getByRole('button', { name: 'All', pressed: true })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Text' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Rich Text' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Image' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'File' })).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Image' }))

    expect(onChange).toHaveBeenCalledWith(Filter.Image)
  })

  it('uses the All control to reset the type filter', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()

    render(<QuickPanelTypeFilterBar activeFilter={Filter.File} onChange={onChange} />)

    await user.click(screen.getByRole('button', { name: 'All' }))

    expect(onChange).toHaveBeenCalledWith(Filter.All)
  })
})
