import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

describe('shared select', () => {
  it('keeps rich labels and selects by keyboard without selecting disabled options', async () => {
    const user = userEvent.setup()
    const onValueChange = vi.fn()
    render(
      <Select defaultValue="light" onValueChange={onValueChange}>
        <SelectTrigger aria-label="Theme">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="light">
            <span>Light theme</span>
          </SelectItem>
          <SelectItem value="disabled" disabled>
            Unavailable
          </SelectItem>
          <SelectItem value="dark">Dark theme</SelectItem>
        </SelectContent>
      </Select>
    )
    const trigger = screen.getByRole('combobox', { name: 'Theme' })
    expect(trigger).toHaveTextContent('Light theme')
    await user.click(trigger)
    await screen.findByRole('option', { name: 'Dark theme' })
    await user.keyboard('{ArrowDown}{Enter}')
    expect(onValueChange).not.toHaveBeenCalled()
    await user.keyboard('{ArrowDown}{Enter}')
    await waitFor(() => expect(onValueChange).toHaveBeenCalledWith('dark'))
    await waitFor(() => expect(trigger).toHaveTextContent('Dark theme'))
    await waitFor(() => expect(trigger).toHaveFocus())
  })

  it('closes with Escape without changing the value and leaves disabled triggers closed', async () => {
    const user = userEvent.setup()
    const change = vi.fn()
    const { rerender } = render(
      <Select defaultValue="one" onValueChange={change}>
        <SelectTrigger aria-label="Choice">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="one">One</SelectItem>
        </SelectContent>
      </Select>
    )
    const trigger = screen.getByRole('combobox', { name: 'Choice' })
    await user.click(trigger)
    await screen.findByRole('option')
    await user.keyboard('{Escape}')
    await waitFor(() => expect(trigger).toHaveAttribute('aria-expanded', 'false'))
    expect(change).not.toHaveBeenCalled()
    rerender(
      <Select disabled>
        <SelectTrigger aria-label="Choice">
          <SelectValue placeholder="Choose" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="one">One</SelectItem>
        </SelectContent>
      </Select>
    )
    await user.click(screen.getByRole('combobox', { name: 'Choice' }))
    expect(screen.getByRole('combobox', { name: 'Choice' })).toBeDisabled()
    expect(screen.queryByRole('option')).not.toBeInTheDocument()
  })
})
