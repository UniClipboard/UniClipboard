import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { EntrySourceView } from '@/api/tauri-command/clipboard_delivery'
import ClipboardActionBar from '@/components/clipboard/ClipboardActionBar'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

const item: DisplayClipboardItem = { id: 'entry-1', type: 'text', activeTime: 0, content: null }

vi.mock('@/components/clipboard/ClipboardSendMenu', () => ({
  default: ({ entryId, disabled }: { entryId: string; disabled?: boolean }) => (
    <button type="button" disabled={disabled} data-entry-id={entryId}>
      Send
    </button>
  ),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => fallback ?? key,
  }),
}))

describe('ClipboardActionBar', () => {
  it.each<{ source: EntrySourceView; unavailable: boolean; disabled: boolean }>([
    { source: { tag: 'local' }, unavailable: false, disabled: false },
    { source: { tag: 'local' }, unavailable: true, disabled: true },
    {
      source: { tag: 'remote', deviceId: 'peer-1', deviceName: null },
      unavailable: false,
      disabled: true,
    },
    { source: { tag: 'historical' }, unavailable: false, disabled: true },
  ])(
    'preserves send availability for $source.tag, unavailable=$unavailable',
    ({ source, unavailable, disabled }) => {
      render(
        <ClipboardActionBar
          item={{ ...item, isUnavailable: unavailable }}
          delivery={{ entryId: item.id, source, deliveries: [] }}
          copySuccess={false}
          onCopy={vi.fn()}
          onDelete={vi.fn()}
          onToggleFavorite={vi.fn()}
        />
      )

      expect(screen.getAllByRole('button')).toHaveLength(4)
      const send = screen.getByRole('button', { name: 'Send' })
      expect(send).toHaveAttribute('data-entry-id', item.id)
      if (disabled) expect(send).toBeDisabled()
      else expect(send).toBeEnabled()
    }
  )

  it('renders favorite action and toggles the active item', async () => {
    const user = userEvent.setup()
    const onToggleFavorite = vi.fn()

    render(
      <ClipboardActionBar
        item={item}
        delivery={null}
        copySuccess={false}
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
        item={{ ...item, isFavorited: true }}
        delivery={null}
        copySuccess={false}
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
      item={item}
      delivery={null}
      copySuccess={false}
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
      item={item}
      delivery={null}
      copySuccess={false}
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
      item={item}
      delivery={null}
      copySuccess={false}
      onCopy={vi.fn()}
      onDelete={vi.fn()}
      onToggleFavorite={vi.fn()}
    />
  )
  for (const button of screen.getAllByRole('button')) expect(button).not.toHaveAttribute('title')
})
