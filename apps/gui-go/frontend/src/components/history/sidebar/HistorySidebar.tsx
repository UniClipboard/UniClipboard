import { m } from 'framer-motion'
import { Inbox, MonitorCog, Pin, Plus, Settings, Smartphone, Trash2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { NavLink, useNavigate } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import DevProfileIndicator from '@/components/DevProfileIndicator'
import { useTagTints } from '@/components/history/tags/tag-colors-context'
import { ThemeModeSwitch } from '@/components/motion/theme-mode-switch'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useLibraryChrome } from '@/contexts/library-chrome-context'
import { useLibraryCounts } from '@/hooks/useLibraryCounts'
import { useMobileDeviceList } from '@/hooks/useMobileDeviceList'
import { usePlatform } from '@/hooks/usePlatform'
import { useWindowDragging } from '@/hooks/useWindowDragging'
import { isMobileDeviceActive } from '@/lib/mobile-device-status'
import { tagLabel } from '@/lib/search-tags'
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
import { sidebarTagRows } from './history-sidebar-tags'
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
import SidebarStatusActions from './SidebarStatusActions'

/** Shared Library sidebar of the History and Devices top-level pages
 * (HSidebar.dc.html). It is the window's left edge and its top-level
 * navigation; on macOS its top strip also clears the traffic lights. It can
 * collapse to an icon column under a top band (toggle, ⌃⌘S); hovering the
 * collapsed toggle peeks the full sidebar over the content. In the compact
 * window tier it is collapsed and the toggle pins that overlay as a drawer.
 * Smart Views are
 * out of scope this round (exec plan, "已决定"); Tags list the daemon's
 * sidebar tags in order, with `+` and "All tags" opening the tag Library. */
function HistorySidebar(props: HistorySidebarProps) {
  const { context } = props
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { isMac } = usePlatform()
  const windowDragging = useWindowDragging()
  const chrome = useLibraryChrome()
  const hidden = chrome.hidden
  const { closeDrawer, drawerOpen, setLightsInContent } = chrome
  // Smart Views are out of scope this round; restore with the section below.
  // const [smartViewsOpen, setSmartViewsOpen] = useState(true)
  const [tagsOpen, setTagsOpen] = useState(true)
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

  const tagRows = props.sidebarTagIds && sidebarTagRows(props.sidebarTagIds, props.tags)
  const tintOf = useTagTints()
  const libraryCounts = useLibraryCounts(
    props.context === 'history' ? props.countsRevision : undefined
  )
  const countBadge = (count: number | undefined) =>
    count === undefined ? null : (
      <span className="shrink-0 text-ui-caption text-muted-foreground tabular-nums">
        {count.toLocaleString()}
      </span>
    )

  // A section's empty hint (Tags now, Smart Views once they return).
  const emptyHintClass =
    'mx-1.5 rounded-lg border border-dashed border-border p-2.5 text-ui-caption text-muted-foreground'

  // Collapsed, the macOS traffic lights drop into the top band, level with the
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
  const tagLibrary =
    props.context === 'history' && props.tagLibrary
      ? {
          total: props.tagLibrary.total,
          // A keyboard-activated click (`detail` 0) gets focus back on close;
          // a pointer click leaves focus alone, as WebKit never focused it.
          open: (event: React.MouseEvent<HTMLElement>) => {
            const returnFocusTo = event.detail === 0 ? event.currentTarget : null
            closeOverlay()
            props.tagLibrary?.open(returnFocusTo)
          },
        }
      : null

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
    <aside className="flex h-full w-55 shrink-0 flex-col text-sidebar-foreground">
      {variant === 'inline' ? (
        // Top strip, draggable. On macOS it clears the traffic lights and the
        // toggle follows them on their row.
        <div
          data-tauri-drag-region
          {...windowDragging}
          className={cn('flex h-11 shrink-0 items-start pt-2.5', isMac ? 'pl-20' : 'pl-3')}
        >
          <LibraryToggleButton />
        </div>
      ) : (
        <div className="mb-1 mt-2.75 flex h-8.5 shrink-0 items-center pl-3">
          <LibraryToggleButton shared={false} />
        </div>
      )}
      <ScrollArea className="min-h-0 flex-1">
        <nav aria-label={t('history.sidebar.library')} className="flex flex-col px-2.5 pb-3">
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
            trailing={
              tagLibrary ? (
                <button
                  type="button"
                  aria-label={t('history.tags.manage')}
                  onClick={tagLibrary.open}
                  className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/5 hover:text-foreground outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50"
                >
                  <Plus className="size-3" strokeWidth={3} aria-hidden="true" />
                </button>
              ) : undefined
            }
          >
            {tagRows?.length === 0 ? (
              <p className={emptyHintClass}>{t('history.tags.sidebarEmpty')}</p>
            ) : (
              tagRows?.map(row => (
                <HistorySidebarNavItem
                  key={row.id}
                  leading={
                    <span
                      style={tintOf(row.id).style}
                      className={cn('size-2 rounded-full', tintOf(row.id).dot)}
                    />
                  }
                  label={`#${tagLabel(row, t)}`}
                  active={activeTag === row.id}
                  onClick={() => selectTag(row.id)}
                  // An empty row shows no count, as does a locked session,
                  // whose tag counts are unknown.
                  trailing={countBadge(row.count || undefined)}
                />
              ))
            )}
            {tagLibrary && tagLibrary.total > 0 && (
              <button
                type="button"
                onClick={tagLibrary.open}
                className="flex h-7 items-center rounded-lg pl-8.5 pr-2.5 text-left text-ui-body text-muted-foreground hover:bg-foreground/5 hover:text-foreground outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50"
              >
                {t('history.tags.allTags', { n: tagLibrary.total })}
              </button>
            )}
          </HistorySidebarSection>

          <HistorySidebarSection
            label={t('history.sidebar.devices')}
            open={devicesOpen}
            onOpenChange={setDevicesOpen}
            trailing={
              context === 'history' ? (
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

      <div className="flex items-center gap-1 px-2.5 pt-2 empty:hidden">
        <SidebarStatusActions />
      </div>
      <div className="flex items-center justify-between gap-2 px-2.5 py-2">
        <NavLink
          to="/settings"
          className="flex h-7.5 min-w-0 flex-1 items-center gap-2.5 rounded-md px-2.5 py-1 text-ui-body text-sidebar-foreground hover:bg-foreground/5"
        >
          <Settings className="size-3.5 shrink-0" aria-hidden="true" />
          <span className="truncate">{t('history.sidebar.settings')}</span>
        </NavLink>
        <DevProfileIndicator compact />
        <ThemeModeSwitch className="shrink-0" />
      </div>
    </aside>
  )

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
        className="absolute inset-x-0 bottom-0 border-r border-t border-border bg-sidebar data-[band=false]:border-t-0"
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
