import { m } from 'framer-motion'
import { Inbox, MonitorCog, Pin, Settings, Smartphone, Trash2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { NavLink, useNavigate } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import { ThemeModeSwitch } from '@/components/motion/theme-mode-switch'
import { ThemeToggle } from '@/components/motion/theme-toggle'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useLibraryChrome } from '@/contexts/library-chrome-context'
import { useSidebarSlot } from '@/contexts/sidebar-slot-context'
import { useLibraryCounts } from '@/hooks/useLibraryCounts'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import { useWindowDragging } from '@/hooks/useWindowDragging'
import { isMobileDeviceActive } from '@/lib/mobile-device-status'
import { cn } from '@/lib/utils'
import { useAppSelector } from '@/store/hooks'
import {
  LIBRARY_SIDEBAR_TRANSITION,
  SIDEBAR_BAND_HEIGHT,
  SIDEBAR_PANEL_WIDTH,
  SIDEBAR_PEEK_DELAY_MS,
  SIDEBAR_PEEK_TRANSITION,
  SIDEBAR_RAIL_WIDTH,
} from './history-sidebar-motion'
import { sidebarTagRows, tagDotClass } from './history-sidebar-tags'
import {
  HISTORY_LIBRARY_FILTER_STATE,
  HISTORY_TAG_FILTER_STATE,
  type HistorySidebarProps,
  type SidebarLibraryFilter,
} from './history-sidebar-types'
import HistorySidebarNavItem from './HistorySidebarNavItem'
import HistorySidebarRail from './HistorySidebarRail'
import HistorySidebarSection from './HistorySidebarSection'
import { LibraryToggleButton } from './LibraryToggle'

/** Shared Library sidebar of the History and Devices top-level pages
 * (HSidebar.dc.html). On macOS it is the window's left edge: it owns the
 * traffic-light strip and replaces the icon rail as top-level navigation. It can
 * collapse to an icon column under a top band (toggle, ⌃⌘S); hovering the
 * collapsed toggle peeks the full sidebar over the content. In the compact
 * window tier it is collapsed and the toggle pins that overlay as a drawer.
 * Smart Views are
 * out of scope this round (exec plan, "已决定"); Tags always list the builtin
 * tags and the `file` content type, then any custom tag in use. */
function HistorySidebar(props: HistorySidebarProps) {
  const { context } = props
  const { t } = useTranslation()
  const navigate = useNavigate()
  // On macOS the sidebar is the window's left edge and top-level navigation;
  // on Windows/Linux the icon rail is, and the sidebar stays a Library panel.
  const { libraryOwnsNavigation } = useSidebarSlot()
  const windowDragging = useWindowDragging()
  const chrome = useLibraryChrome()
  const hidden = libraryOwnsNavigation && chrome.hidden
  const { closeDrawer, drawerOpen, setLightsInContent } = chrome
  // Smart Views are out of scope this round; restore with the section below.
  // const [smartViewsOpen, setSmartViewsOpen] = useState(libraryOwnsNavigation)
  const [tagsOpen, setTagsOpen] = useState(libraryOwnsNavigation)
  const [devicesOpen, setDevicesOpen] = useState(context === 'devices')

  const spaceMembers = useAppSelector(state => state.devices.spaceMembers)
  const mobileDevices = useMobileDeviceList()
  const now = Date.now()
  const devices = [
    ...spaceMembers.map(m => ({
      id: m.peerId,
      name: m.deviceName,
      online: m.connected,
      mobile: false,
    })),
    ...mobileDevices.map(d => ({
      id: d.deviceId,
      name: d.label,
      online: isMobileDeviceActive(d, now),
      mobile: true,
    })),
  ]
  const onlineCount = devices.filter(d => d.online).length

  const tagRows = sidebarTagRows(props.tags)
  const libraryCounts = useLibraryCounts(
    props.context === 'history' ? props.countsRevision : undefined
  )
  const countBadge = (count: number | undefined) =>
    count === undefined ? null : (
      <span className="shrink-0 text-ui-caption text-muted-foreground tabular-nums">
        {count.toLocaleString()}
      </span>
    )

  // Smart Views' empty hint; restore with the section below.
  // const emptyHintClass = libraryOwnsNavigation
  //   ? 'mx-1.5 rounded-lg border border-dashed border-border p-2.5 text-ui-caption text-muted-foreground'
  //   : 'px-2.5 py-1.5 text-ui-caption text-muted-foreground/70'

  // Collapsed, the traffic lights drop into the top band, level with the
  // content header; expanded, they sit over the sidebar.
  useEffect(() => {
    setLightsInContent(hidden)
    return () => setLightsInContent(false)
  }, [hidden, setLightsInContent])

  // Peek: hovering the collapsed toggle shows the full sidebar over the
  // content until the pointer leaves it; clicking the toggle pins it.
  const [peek, setPeek] = useState(false)
  const peekTimer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const cancelPeekTimer = () => clearTimeout(peekTimer.current)
  const schedulePeek = () => {
    cancelPeekTimer()
    peekTimer.current = setTimeout(() => setPeek(true), SIDEBAR_PEEK_DELAY_MS)
  }
  const closePeek = () => {
    cancelPeekTimer()
    setPeek(false)
  }
  useEffect(() => () => clearTimeout(peekTimer.current), [])
  // Showing or hiding the sidebar ends a peek.
  useEffect(() => setPeek(false), [hidden, drawerOpen])

  const asDrawer = hidden && drawerOpen
  const overlayOpen = hidden && (drawerOpen || peek)
  useEffect(() => {
    if (!overlayOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      closeDrawer()
      setPeek(false)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [overlayOpen, closeDrawer])

  const closeOverlay = () => {
    closeDrawer()
    closePeek()
  }
  const selectLibrary = (filter: SidebarLibraryFilter) => {
    closeOverlay()
    if (props.context === 'history') props.onSelectLibrary(filter)
    else navigate('/history', { state: { [HISTORY_LIBRARY_FILTER_STATE]: filter } })
  }
  const libraryActive = (filter: Filter) =>
    props.context === 'history' && props.activeFilter === filter

  const activeTag = props.context === 'history' ? props.activeTag : null
  // Picking the active tag again clears it; the tag filter is independent of
  // the Library row, so neither resets the other.
  const selectTag = (tag: string) => {
    closeOverlay()
    if (props.context === 'history') props.onSelectTag(activeTag === tag ? null : tag)
    else navigate('/history', { state: { [HISTORY_TAG_FILTER_STATE]: tag } })
  }
  // The `file` row is a content type: picking it again goes back to All items.
  const selectFile = () => selectLibrary(libraryActive(Filter.File) ? Filter.All : Filter.File)

  // Collapsed: an icon column; the full sidebar opens over it (peek or
  // compact-tier drawer), so the columns beside it never shift.
  const rail = hidden ? (
    <HistorySidebarRail
      context={context}
      libraryActive={libraryActive}
      onSelectLibrary={selectLibrary}
      onTogglePointerEnter={schedulePeek}
      onTogglePointerLeave={cancelPeekTimer}
    />
  ) : null

  // `inline`: the sidebar column itself. `overlay`: the same panel floating
  // over the content under the top band, its toggle exactly over the rail's.
  const renderPanel = (variant: 'inline' | 'overlay') => (
    <aside
      className={cn(
        'flex h-full shrink-0 flex-col',
        !libraryOwnsNavigation
          ? 'w-56 border-r border-border/50 bg-muted/15 xl:w-60'
          : variant === 'inline'
            ? 'w-55 text-sidebar-foreground'
            : 'w-55 text-sidebar-foreground'
      )}
    >
      {libraryOwnsNavigation &&
        (variant === 'inline' ? (
          // Traffic-light strip; the toggle follows the lights on their row.
          <div
            data-tauri-drag-region
            {...windowDragging}
            className="flex h-11 shrink-0 items-start pt-2.5 pl-20"
          >
            <LibraryToggleButton />
          </div>
        ) : (
          <div className="mb-1 mt-2.75 flex h-8.5 shrink-0 items-center pl-3">
            <LibraryToggleButton shared={false} />
          </div>
        ))}
      <ScrollArea className="min-h-0 flex-1">
        <nav
          aria-label={t('history.sidebar.library')}
          className={cn('flex flex-col pb-3', libraryOwnsNavigation ? 'px-2.5' : 'px-2 pt-2')}
        >
          <HistorySidebarNavItem
            icon={Inbox}
            label={t('history.sidebar.allItems')}
            active={libraryActive(Filter.All)}
            onClick={() => selectLibrary(Filter.All)}
            trailing={countBadge(libraryCounts?.all)}
          />
          <HistorySidebarNavItem
            icon={Pin}
            label={t('history.sidebar.pinned')}
            active={libraryActive(Filter.Favorited)}
            onClick={() => selectLibrary(Filter.Favorited)}
            trailing={countBadge(libraryCounts?.pinned)}
          />
          <HistorySidebarNavItem
            icon={Trash2}
            label={t('history.sidebar.recentlyDeleted')}
            disabled
          />

          {/* Smart Views are out of scope this round.
          <HistorySidebarSection
            label={t('history.sidebar.smartViews')}
            open={smartViewsOpen}
            onOpenChange={setSmartViewsOpen}
          >
            <p className={emptyHintClass}>{t('history.sidebar.smartViewsEmpty')}</p>
          </HistorySidebarSection>
          */}

          <HistorySidebarSection
            label={t('history.sidebar.tags')}
            open={tagsOpen}
            onOpenChange={setTagsOpen}
          >
            {tagRows.map(row => (
              <HistorySidebarNavItem
                key={row.id}
                leading={<span className={cn('size-2 rounded-full', tagDotClass(row.id))} />}
                label={`#${t(`history.type.${row.id}`, { defaultValue: row.id })}`}
                active={row.kind === 'file' ? libraryActive(Filter.File) : activeTag === row.id}
                onClick={row.kind === 'file' ? selectFile : () => selectTag(row.id)}
                // An empty builtin row shows no count, as does a locked
                // session, whose tag counts are unknown.
                trailing={countBadge(row.count || undefined)}
              />
            ))}
          </HistorySidebarSection>

          <HistorySidebarSection
            label={t('history.sidebar.devices')}
            open={devicesOpen}
            onOpenChange={setDevicesOpen}
            trailing={
              context === 'history' && libraryOwnsNavigation ? (
                <NavLink
                  to="/devices"
                  className="shrink-0 text-ui-caption font-medium text-primary hover:underline"
                >
                  {t('history.sidebar.manageDevices')}
                </NavLink>
              ) : (
                <span className="shrink-0 text-ui-caption text-muted-foreground/70">
                  {t('history.sidebar.devicesOnline', {
                    online: onlineCount,
                    total: devices.length,
                  })}
                </span>
              )
            }
          >
            {devices.length === 0 ? (
              <p className="px-2.5 py-1.5 text-ui-caption text-muted-foreground/70">
                {t('history.sidebar.devicesEmpty')}
              </p>
            ) : (
              devices.map(device => (
                <HistorySidebarNavItem
                  key={device.id}
                  icon={device.mobile ? Smartphone : MonitorCog}
                  label={device.name}
                  trailing={
                    <span
                      aria-hidden="true"
                      className={cn(
                        'size-1.5 shrink-0 rounded-full',
                        device.online ? 'bg-success' : 'bg-muted-foreground/40'
                      )}
                    />
                  }
                />
              ))
            )}
          </HistorySidebarSection>
        </nav>
      </ScrollArea>

      <div
        className={cn(
          'flex items-center justify-between gap-2 py-2',
          libraryOwnsNavigation ? 'px-2.5' : 'border-t border-border/50 px-3'
        )}
      >
        <NavLink
          to="/settings"
          className={cn(
            'flex min-w-0 items-center gap-2 rounded-md py-1 text-ui-body text-muted-foreground hover:text-foreground',
            libraryOwnsNavigation
              ? 'h-7.5 flex-1 gap-2.5 px-2.5 text-sidebar-foreground hover:bg-foreground/5'
              : 'px-1.5'
          )}
        >
          <Settings className="size-3.5 shrink-0" aria-hidden="true" />
          <span className="truncate">{t('history.sidebar.settings')}</span>
        </NavLink>
        {libraryOwnsNavigation ? (
          <ThemeModeSwitch className="shrink-0" />
        ) : (
          <ThemeToggle className="size-7 shrink-0 rounded-md text-muted-foreground hover:bg-muted/60 hover:text-foreground" />
        )}
      </div>
    </aside>
  )

  if (!libraryOwnsNavigation) return renderPanel('inline')

  // The inline column. Its width eases between the full sidebar and the icon
  // column in step with the toggle's glide. The surface (background and the
  // border on the moving edge) is a separate layer that starts below the top
  // band while collapsed, so the band stays bare for the traffic lights.
  const column = (
    <m.div
      initial={false}
      animate={{ width: hidden ? SIDEBAR_RAIL_WIDTH : SIDEBAR_PANEL_WIDTH }}
      transition={LIBRARY_SIDEBAR_TRANSITION}
      className="relative h-full shrink-0"
    >
      <m.div
        aria-hidden="true"
        initial={false}
        animate={{
          top: hidden ? SIDEBAR_BAND_HEIGHT : 0,
          borderTopRightRadius: hidden ? 12 : 0,
        }}
        transition={LIBRARY_SIDEBAR_TRANSITION}
        className="absolute inset-x-0 bottom-0 border-r border-t border-sidebar-border bg-sidebar data-[band=false]:border-t-0"
        data-band={hidden}
      />
      <div className="relative h-full overflow-hidden">{rail ?? renderPanel('inline')}</div>
    </m.div>
  )
  if (!overlayOpen) return column
  return (
    <>
      {column}
      {asDrawer && (
        <div
          aria-hidden="true"
          onClick={closeDrawer}
          className="fixed inset-0 z-40 bg-black/10 animate-in fade-in-0 duration-150"
        />
      )}
      {/* The overlay grows out of the icon column: its width eases from the
          rail's to the sidebar's (SIDEBAR_PEEK_TRANSITION). It carries the
          surface (background, border, corners, shadow) itself and animates
          width only. Not tw-animate's `animate-in`: that also animates
          `filter`, and WebKit clips a filtering layer to its own box, so a
          shadow outside it would only appear once the animation ended. */}
      <m.div
        data-library-overlay=""
        data-library-drawer={asDrawer ? '' : undefined}
        onPointerLeave={asDrawer ? undefined : closePeek}
        initial={{ width: SIDEBAR_RAIL_WIDTH }}
        animate={{ width: SIDEBAR_PANEL_WIDTH }}
        transition={SIDEBAR_PEEK_TRANSITION}
        className="fixed bottom-2 left-0 top-16 z-50 overflow-hidden rounded-r-xl border border-l-0 border-sidebar-border bg-sidebar shadow-xl"
      >
        {renderPanel('overlay')}
      </m.div>
    </>
  )
}

export default HistorySidebar
