import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import ClipboardActionBar from '@/components/clipboard/ClipboardActionBar'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => fallback ?? key,
  }),
}))

describe('ClipboardActionBar', () => {
  it('renders favorite action and toggles the active item', async () => {
    const user = userEvent.setup()
    const onToggleFavorite = vi.fn()

    render(
      <ClipboardActionBar
        hasActiveItem
        copySuccess={false}
        isFavorited={false}
        onCopy={vi.fn()}
        onDelete={vi.fn()}
        onToggleFavorite={onToggleFavorite}
      />
    )

    const favoriteButton = screen.getByRole('button', { name: 'clipboard.actionBar.favorite' })

    expect(favoriteButton).toBeEnabled()
    expect(screen.getByText('F')).toBeInTheDocument()

    await user.click(favoriteButton)

    expect(onToggleFavorite).toHaveBeenCalledTimes(1)
  })

  it('labels an already favorited item as unfavorite', () => {
    render(
      <ClipboardActionBar
        hasActiveItem
        copySuccess={false}
        isFavorited
        onCopy={vi.fn()}
        onDelete={vi.fn()}
        onToggleFavorite={vi.fn()}
      />
    )

    expect(
      screen.getByRole('button', { name: 'clipboard.actionBar.unfavorite' })
    ).toBeInTheDocument()
  })
})

it('reveals labels on hover and keeps them visible while keyboard focus stays inside', async () => {
  render(
    <ClipboardActionBar
      hasActiveItem
      copySuccess={false}
      isFavorited={false}
      onCopy={vi.fn()}
      onDelete={vi.fn()}
      onToggleFavorite={vi.fn()}
    />
  )
  const copy = screen.getByRole('button', { name: 'clipboard.actionBar.copy' })
  const label = screen.getByText('clipboard.actionBar.copy')
  expect(label).toHaveAttribute('aria-hidden', 'true')
  fireEvent.pointerEnter(copy, { pointerType: 'mouse' })
  await waitFor(() => expect(label).toHaveAttribute('aria-hidden', 'false'))
  fireEvent.focus(copy)
  fireEvent.pointerLeave(copy, { pointerType: 'mouse' })
  await new Promise(resolve => setTimeout(resolve, 150))
  expect(label).toHaveAttribute('aria-hidden', 'false')
  fireEvent.blur(copy, { relatedTarget: document.body })
  await waitFor(() => expect(label).toHaveAttribute('aria-hidden', 'true'))
})

it('expands only the pointed action', async () => {
  render(
    <ClipboardActionBar
      hasActiveItem
      copySuccess={false}
      isFavorited={false}
      onCopy={vi.fn()}
      onDelete={vi.fn()}
      onToggleFavorite={vi.fn()}
    />
  )
  fireEvent.pointerEnter(screen.getByRole('button', { name: 'clipboard.actionBar.copy' }), {
    pointerType: 'mouse',
  })
  await waitFor(() =>
    expect(screen.getByText('clipboard.actionBar.copy')).toHaveAttribute('aria-hidden', 'false')
  )
  expect(screen.getByText('clipboard.actionBar.favorite')).toHaveAttribute('aria-hidden', 'true')
  expect(screen.getByText('clipboard.actionBar.delete')).toHaveAttribute('aria-hidden', 'true')
})

it('does not attach native tooltips to actions', () => {
  render(
    <ClipboardActionBar
      hasActiveItem
      copySuccess={false}
      isFavorited={false}
      onCopy={vi.fn()}
      onDelete={vi.fn()}
      onToggleFavorite={vi.fn()}
    />
  )
  for (const button of screen.getAllByRole('button')) expect(button).not.toHaveAttribute('title')
})
