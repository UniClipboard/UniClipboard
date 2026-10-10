import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import TagColorPicker from '../TagColorPicker'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

describe('TagColorPicker', () => {
  it('picks a palette color', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()
    render(<TagColorPicker value="orange" onChange={onChange} />)

    expect(screen.getByRole('radio', { name: 'history.tags.colors.orange' })).toHaveAttribute(
      'aria-checked',
      'true'
    )
    await user.click(screen.getByRole('radio', { name: 'history.tags.colors.green' }))
    expect(onChange).toHaveBeenCalledWith('green')
  })

  it('takes any #rrggbb typed in, with or without the #, and ignores partial input', () => {
    const onChange = vi.fn()
    render(<TagColorPicker value={undefined} onChange={onChange} />)
    const hex = screen.getByRole('textbox', { name: 'history.tags.hexColor' })

    fireEvent.change(hex, { target: { value: '#c04' } })
    expect(onChange).not.toHaveBeenCalled()
    fireEvent.change(hex, { target: { value: 'C0467A' } })
    expect(onChange).toHaveBeenLastCalledWith('#c0467a')
  })

  it('shows a custom color in the hex field and leaves the palette unchecked', () => {
    render(<TagColorPicker value="#c0467a" onChange={vi.fn()} />)

    expect(screen.getByRole('textbox', { name: 'history.tags.hexColor' })).toHaveValue('#c0467a')
    for (const radio of screen.getAllByRole('radio')) {
      expect(radio).toHaveAttribute('aria-checked', 'false')
    }
  })
})
