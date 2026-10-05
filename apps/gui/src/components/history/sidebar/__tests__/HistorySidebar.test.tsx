import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import { type LibraryChrome, LibraryChromeContext } from '@/contexts/library-chrome-context'
import { SidebarSlotContext } from '@/contexts/sidebar-slot-context'
import type { SearchTagOption } from '@/lib/search-tags'
import HistorySidebar from '../HistorySidebar'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string; count?: number }) =>
      options?.count !== undefined ? `${key}:${options.count}` : (options?.defaultValue ?? key),
  }),
}))
vi.mock('@/store/hooks', () => ({ useAppSelector: () => [] }))
vi.mock('@/hooks/useMobileDeviceList', () => ({ useMobileDeviceList: () => [] }))
vi.mock('@/hooks/useWindowDragging', () => ({ useWindowDragging: () => ({}) }))
const libraryCounts = vi.hoisted(() => ({ value: null as { all: number; pinned: number } | null }))
vi.mock('@/hooks/useLibraryCounts', () => ({ useLibraryCounts: () => libraryCounts.value }))
vi.mock('@/components/motion/theme-mode-switch', () => ({ ThemeModeSwitch: () => null }))
vi.mock('@/components/motion/theme-toggle', () => ({ ThemeToggle: () => null }))

function chrome(overrides: Partial<LibraryChrome> = {}): LibraryChrome {
  return {
    hidden: false,
    drawer: false,
    drawerOpen: false,
    toggle: vi.fn(),
    closeDrawer: vi.fn(),
    setLightsInContent: vi.fn(),
    ...overrides,
  }
}

function tag(id: string, count: number, isBuiltin = false): SearchTagOption {
  return { id, count, isBuiltin }
}

function renderSidebar(
  libraryOwnsNavigation: boolean,
  value: LibraryChrome = chrome(),
  {
    tags = [],
    activeTag = null,
    activeFilter = Filter.All,
    tagLibrary,
    sidebarTagIds = null,
  }: {
    tags?: SearchTagOption[]
    sidebarTagIds?: readonly string[] | null
    activeTag?: string | null
    activeFilter?: Filter
    tagLibrary?: { total: number; open: () => void }
  } = {}
) {
  const onSelectLibrary = vi.fn()
  const onSelectTag = vi.fn()
  const view = render(
    <MemoryRouter>
      <SidebarSlotContext value={{ contentToolbarHost: null, libraryOwnsNavigation }}>
        <LibraryChromeContext value={value}>
          <HistorySidebar
            context="history"
            activeFilter={activeFilter}
            onSelectLibrary={onSelectLibrary}
            tags={tags}
            sidebarTagIds={sidebarTagIds}
            activeTag={activeTag}
            onSelectTag={onSelectTag}
            tagLibrary={tagLibrary}
            countsRevision={null}
          />
        </LibraryChromeContext>
      </SidebarSlotContext>
    </MemoryRouter>
  )
  return { onSelectLibrary, onSelectTag, ...view }
}

describe('HistorySidebar show/hide (macOS)', () => {
  it('shows inline with a Hide sidebar toggle in the traffic-light strip', async () => {
    const user = userEvent.setup()
    const value = chrome()
    renderSidebar(true, value)

    expect(screen.getByRole('navigation')).toBeInTheDocument()
    expect(document.querySelector('[data-library-drawer]')).toBeNull()
    expect(value.setLightsInContent).toHaveBeenLastCalledWith(false)
    await user.click(screen.getByRole('button', { name: 'history.sidebar.hide' }))
    expect(value.toggle).toHaveBeenCalledTimes(1)
  })

  it('collapses to the icon column, led by the Show sidebar toggle', async () => {
    const user = userEvent.setup()
    const value = chrome({ hidden: true })
    const { onSelectLibrary } = renderSidebar(true, value)

    expect(document.querySelector('aside')?.dataset.sidebar).toBe('rail')
    expect(screen.getByRole('button', { name: 'history.sidebar.allItems' })).toHaveAttribute(
      'aria-current',
      'true'
    )
    expect(screen.getByRole('link', { name: 'history.sidebar.devices' })).toHaveAttribute(
      'href',
      '/devices'
    )
    // The lights drop into the top band, level with the content header.
    expect(value.setLightsInContent).toHaveBeenLastCalledWith(true)
    const rail = document.querySelector<HTMLElement>('[data-sidebar="rail"]')!
    const show = within(rail).getByRole('button', { name: 'history.sidebar.show' })
    // The toggle comes first in the column, ahead of the Library icons.
    expect(within(rail).getAllByRole('button')[0]).toBe(show)
    await user.click(show)
    expect(value.toggle).toHaveBeenCalledTimes(1)
    await user.click(screen.getByRole('button', { name: 'history.sidebar.pinned' }))
    expect(onSelectLibrary).toHaveBeenCalledWith(Filter.Favorited)
  })

  it('peeks the full sidebar while the collapsed toggle is hovered', async () => {
    const user = userEvent.setup()
    const value = chrome({ hidden: true })
    renderSidebar(true, value)

    const rail = document.querySelector<HTMLElement>('[data-sidebar="rail"]')!
    await user.hover(within(rail).getByRole('button', { name: 'history.sidebar.show' }))
    const overlay = await waitFor(() => {
      const element = document.querySelector<HTMLElement>('[data-library-overlay]')
      expect(element).not.toBeNull()
      return element!
    })
    // A peek is not the compact drawer: no backdrop, nothing pinned.
    expect(overlay).not.toHaveAttribute('data-library-drawer')
    // jsdom does not run the fade-in, so check presence rather than visibility.
    expect(
      within(overlay).getByRole('button', { name: 'history.sidebar.allItems' })
    ).toBeInTheDocument()

    // jsdom has no geometry, so leave the overlay directly.
    fireEvent.pointerLeave(overlay)
    expect(document.querySelector('[data-library-overlay]')).toBeNull()
    expect(value.toggle).not.toHaveBeenCalled()
  })

  it('opens as a drawer in the compact tier and closes on Escape or a selection', async () => {
    const user = userEvent.setup()
    const value = chrome({ hidden: true, drawer: true, drawerOpen: true })
    const { onSelectLibrary } = renderSidebar(true, value)

    // The drawer opens over the icon column, which stays in place.
    expect(document.querySelector('[data-library-drawer]')).not.toBeNull()
    expect(document.querySelector('[data-sidebar="rail"]')).not.toBeNull()
    await user.keyboard('{Escape}')
    expect(value.closeDrawer).toHaveBeenCalledTimes(1)

    const drawer = document.querySelector<HTMLElement>('[data-library-drawer]')!
    await user.click(within(drawer).getByRole('button', { name: 'history.sidebar.pinned' }))
    expect(value.closeDrawer).toHaveBeenCalledTimes(2)
    expect(onSelectLibrary).toHaveBeenCalledWith(Filter.Favorited)
  })

  it('keeps the plain Library panel on Windows and Linux', () => {
    renderSidebar(false, chrome({ hidden: true }))

    expect(screen.getByRole('navigation')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'history.sidebar.hide' })).toBeNull()
  })
})

describe('HistorySidebar tags', () => {
  it('lists the layout sidebar tags in its order, at most six', () => {
    renderSidebar(true, chrome(), {
      tags: [tag('link', 3, true), tag('code', 0, true), tag('docker', 38), tag('work', 57)],
      sidebarTagIds: ['docker', 'link', 'code', 'image', 'directory', 'work', 'extra'],
    })

    const rows = screen.getAllByRole('button', { name: /^#/ })
    expect(rows.map(row => row.textContent)).toEqual([
      '#docker38',
      '#link3',
      '#code',
      '#image',
      '#directory',
      '#work57',
    ])
  })

  it('shows a hint when the sidebar holds no tag', () => {
    renderSidebar(true, chrome(), { tags: [tag('work', 5)], sidebarTagIds: [] })

    expect(screen.queryAllByRole('button', { name: /^#/ })).toEqual([])
    expect(screen.getByText('history.tags.sidebarEmpty')).toBeInTheDocument()
  })

  it('shows neither rows nor the hint while the layout is unknown', () => {
    renderSidebar(true, chrome(), { tags: [tag('work', 5)], sidebarTagIds: null })

    expect(screen.queryAllByRole('button', { name: /^#/ })).toEqual([])
    expect(screen.queryByText('history.tags.sidebarEmpty')).toBeNull()
  })

  it('opens the tag Library from the + icon and from All tags', async () => {
    const user = userEvent.setup()
    const open = vi.fn()
    renderSidebar(true, chrome(), {
      tags: [tag('work', 57)],
      sidebarTagIds: ['work'],
      tagLibrary: { total: 12, open },
    })

    await user.click(screen.getByRole('button', { name: 'history.tags.manage' }))
    await user.click(screen.getByRole('button', { name: 'history.tags.allTags' }))
    expect(open).toHaveBeenCalledTimes(2)
  })

  it('asks for focus back on close only when the Library was opened by keyboard', async () => {
    const user = userEvent.setup()
    const open = vi.fn()
    renderSidebar(true, chrome(), {
      tags: [tag('work', 57)],
      sidebarTagIds: ['work'],
      tagLibrary: { total: 12, open },
    })
    const plus = screen.getByRole('button', { name: 'history.tags.manage' })

    await user.click(plus)
    expect(open).toHaveBeenLastCalledWith(null)
    plus.focus()
    await user.keyboard('{Enter}')
    expect(open).toHaveBeenLastCalledWith(plus)
  })

  it('offers no Library entry while local tags are unavailable', () => {
    renderSidebar(true, chrome(), { tags: [tag('work', 57)] })

    expect(screen.queryByRole('button', { name: 'history.tags.manage' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'history.tags.allTags' })).toBeNull()
  })

  it('filters by a tag and clears it when picked again', async () => {
    const user = userEvent.setup()
    const tags = [tag('work', 57), tag('docker', 38)]
    const { onSelectTag, rerender } = renderSidebar(true, chrome(), {
      tags,
      sidebarTagIds: ['work', 'docker'],
    })

    await user.click(screen.getByRole('button', { name: /#docker/ }))
    expect(onSelectTag).toHaveBeenLastCalledWith('docker')

    rerender(
      <MemoryRouter>
        <SidebarSlotContext value={{ contentToolbarHost: null, libraryOwnsNavigation: true }}>
          <LibraryChromeContext value={chrome()}>
            <HistorySidebar
              context="history"
              activeFilter={Filter.All}
              onSelectLibrary={vi.fn()}
              tags={tags}
              sidebarTagIds={['work', 'docker']}
              activeTag="docker"
              onSelectTag={onSelectTag}
              countsRevision={null}
            />
          </LibraryChromeContext>
        </SidebarSlotContext>
      </MemoryRouter>
    )
    const active = screen.getByRole('button', { name: /#docker/ })
    expect(active).toHaveAttribute('aria-current', 'true')
    await user.click(active)
    expect(onSelectTag).toHaveBeenLastCalledWith(null)
  })
})

describe('HistorySidebar library counts', () => {
  it('shows the All items and Pinned counts once loaded', () => {
    libraryCounts.value = { all: 12408, pinned: 24 }
    renderSidebar(true)

    expect(screen.getByRole('button', { name: /history.sidebar.allItems/ })).toHaveTextContent(
      '12,408'
    )
    expect(screen.getByRole('button', { name: /history.sidebar.pinned/ })).toHaveTextContent('24')
    libraryCounts.value = null
  })
})
