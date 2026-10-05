import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import HistoryListRow from '../HistoryListRow'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: unknown) =>
      typeof opts === 'object' && opts !== null && 'count' in opts
        ? `${key}:${String(opts.count)}`
        : key,
    i18n: { language: 'en-US' },
  }),
}))

vi.mock('@/store/hooks', () => ({ useAppSelector: () => undefined }))

vi.mock('@/components/history/history-card/useResourceImageUrl', () => ({
  useResourceImageUrl: () => null,
}))

const activeTime = new Date(2026, 8, 27, 22, 14, 8).getTime()

function textItem(overrides: Partial<DisplayClipboardItem> = {}): DisplayClipboardItem {
  return {
    id: 'entry-1',
    type: 'text',
    content: { display_text: 'docker compose up -d', has_detail: false, size: 20, char_count: 20 },
    activeTime,
    ...overrides,
  }
}

function renderRow(
  item: DisplayClipboardItem,
  onClick = vi.fn(),
  deviceName?: string,
  {
    checked = false,
    anyChecked,
    onToggleChecked = vi.fn(),
    tagNames,
  }: {
    checked?: boolean
    anyChecked?: boolean
    onToggleChecked?: () => void
    tagNames?: ReadonlyMap<string, string | null>
  } = {}
) {
  render(
    <HistoryListRow
      item={item}
      deviceName={deviceName}
      checked={checked}
      anyChecked={anyChecked ?? checked}
      onToggleChecked={onToggleChecked}
      copySuccess={false}
      isDeleting={false}
      onClick={onClick}
      onHoverChange={vi.fn()}
      tagNames={tagNames}
    />
  )
  return { onClick, onToggleChecked }
}

describe('HistoryListRow', () => {
  it('shows the one-line title and a meta line of origin device and clock time', () => {
    renderRow(textItem(), vi.fn(), 'arch-desktop')

    expect(screen.getByText('docker compose up -d')).toBeInTheDocument()
    expect(screen.getByText('arch-desktop ·')).toBeInTheDocument()
    expect(screen.getByText('22:14:08')).toBeInTheDocument()
    expect(screen.getByText('TXT')).toBeInTheDocument()
  })

  it('keeps only the clock time when the origin is unknown', () => {
    renderRow(textItem())

    expect(screen.getByText('22:14:08').parentElement?.textContent).toBe('22:14:08')
  })

  it('chips the tags its kind badge does not already show', () => {
    renderRow(textItem({ contentTags: ['code', 'link'] }))

    expect(screen.getByText('#history.type.link')).toBeInTheDocument()
    expect(screen.queryByText('#history.type.code')).toBeNull()
  })

  it('chips its local tags first, two chips at most', () => {
    renderRow(
      textItem({ contentTags: ['code', 'link'], userTagIds: ['t-deploy', 't-docker'] }),
      vi.fn(),
      undefined,
      {
        tagNames: new Map([
          ['t-deploy', 'deploy'],
          ['t-docker', 'docker'],
        ]),
      }
    )

    expect(screen.getByText('#deploy')).toBeInTheDocument()
    expect(screen.getByText('#docker')).toBeInTheDocument()
    expect(screen.queryByText('#history.type.link')).toBeNull()
  })

  it('leaves local tags out until their names are known', () => {
    renderRow(textItem({ contentTags: ['code', 'link'], userTagIds: ['t-deploy'] }))

    expect(screen.queryByText('#t-deploy')).toBeNull()
    expect(screen.getByText('#history.type.link')).toBeInTheDocument()
  })

  it('chips a folder entry as a directory', () => {
    renderRow(
      textItem({
        type: 'file',
        isDirectory: true,
        content: { file_names: ['assets'], file_sizes: [-1] },
      })
    )

    expect(screen.getByText('#history.type.directory')).toBeInTheDocument()
  })

  it('badges code entries and sets their title in the monospace face', () => {
    renderRow(textItem({ contentTags: ['code'] }))

    expect(screen.getByText('</>')).toBeInTheDocument()
    expect(screen.getByText('docker compose up -d').closest('span')).toHaveClass('font-mono')
  })

  it('titles a multi-file entry with the first name and the remaining count', () => {
    renderRow(
      textItem({
        type: 'file',
        content: { file_names: ['invoice-0921.pdf', 'b.pdf', 'c.pdf'], file_sizes: [1, 2, 3] },
      })
    )

    expect(screen.getByText('invoice-0921.pdf +2')).toBeInTheDocument()
    expect(screen.getByText('FILE')).toBeInTheDocument()
  })

  it('marks pinned entries and opens the entry on click', async () => {
    const user = userEvent.setup()
    const { onClick } = renderRow(textItem({ isFavorited: true }))

    expect(screen.getByRole('img', { name: 'history.sidebar.pinned' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'clipboard.item.actions.open' }))
    expect(onClick).toHaveBeenCalledWith('entry-1')
  })

  it('hides its checkbox until some row is checked', () => {
    renderRow(textItem())

    expect(screen.queryByRole('checkbox')).toBeNull()
  })

  it('toggles its bulk checkbox without opening the entry', async () => {
    const user = userEvent.setup()
    const { onClick, onToggleChecked } = renderRow(textItem(), vi.fn(), undefined, {
      anyChecked: true,
    })

    await user.click(screen.getByRole('checkbox', { name: 'history.list.selectItem' }))
    expect(onToggleChecked).toHaveBeenCalledWith('entry-1')
    expect(onClick).not.toHaveBeenCalled()
  })

  it('reflects the checked state', () => {
    renderRow(textItem(), vi.fn(), undefined, { checked: true })

    expect(screen.getByRole('checkbox', { name: 'history.list.selectItem' })).toHaveAttribute(
      'aria-checked',
      'true'
    )
  })

  it('toggles the check on a Command- or Ctrl-click instead of opening', async () => {
    const user = userEvent.setup()
    const { onClick, onToggleChecked } = renderRow(textItem())
    const open = screen.getByRole('button', { name: 'clipboard.item.actions.open' })

    await user.keyboard('{Meta>}')
    await user.click(open)
    await user.keyboard('{/Meta}')
    expect(onToggleChecked).toHaveBeenCalledTimes(1)
    expect(onClick).not.toHaveBeenCalled()
  })

  it('claims the macOS Ctrl-click secondary click before the context menu', () => {
    const { onToggleChecked } = renderRow(textItem())
    const open = screen.getByRole('button', { name: 'clipboard.item.actions.open' })

    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, ctrlKey: true })
    open.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(true)
    expect(onToggleChecked).toHaveBeenCalledWith('entry-1')
    // A click WebKit sends right after it does not toggle again.
    fireEvent.click(open, { ctrlKey: true })
    expect(onToggleChecked).toHaveBeenCalledTimes(1)
  })

  it('leaves a real right-click to the context menu', () => {
    const { onToggleChecked } = renderRow(textItem())
    const open = screen.getByRole('button', { name: 'clipboard.item.actions.open' })

    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 })
    open.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(false)
    expect(onToggleChecked).not.toHaveBeenCalled()
  })
})
