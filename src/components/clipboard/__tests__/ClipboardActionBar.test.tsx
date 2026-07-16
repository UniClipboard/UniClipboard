import { render, screen } from '@testing-library/react'
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
