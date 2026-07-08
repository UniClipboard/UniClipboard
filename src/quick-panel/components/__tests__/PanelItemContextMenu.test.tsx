import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import PanelItemContextMenu from '../PanelItemContextMenu'

// Mock the shared menu so this test targets the wrapper's fallback branch, not
// the menu's Redux/resend dependencies.
vi.mock('@/components/history/HistoryCardContextMenu', () => ({
  default: ({ children }: { children: ReactNode }) => <div data-testid="menu">{children}</div>,
}))

const actions = { onCopy: vi.fn(), onToggleFavorite: vi.fn(), onDelete: vi.fn() }

function richItem(): DisplayClipboardItem {
  return { id: 'e1', type: 'text', content: null, activeTime: 0 }
}

describe('PanelItemContextMenu', () => {
  it('wraps the row in the shared menu when the rich item is present', () => {
    render(
      <PanelItemContextMenu item={richItem()} actions={actions}>
        <div data-testid="row">row</div>
      </PanelItemContextMenu>
    )

    expect(screen.getByTestId('menu')).toBeInTheDocument()
    expect(screen.getByTestId('row')).toBeInTheDocument()
  })

  it('renders the row without a menu when the rich item is missing', () => {
    render(
      <PanelItemContextMenu item={undefined} actions={actions}>
        <div data-testid="row">row</div>
      </PanelItemContextMenu>
    )

    expect(screen.queryByTestId('menu')).not.toBeInTheDocument()
    expect(screen.getByTestId('row')).toBeInTheDocument()
  })
})
