import { LayoutGroup } from 'framer-motion'
import React, { ReactNode, useCallback, useEffect, useId, useMemo, useState } from 'react'
import { useLocation } from 'react-router'
import {
  readLibraryHidden,
  useHistoryLayoutTier,
  writeLibraryHidden,
} from '@/components/history/layout/history-layout'
import InsetSurface from '@/components/layout/InsetSurface'
import SidebarFooter from '@/components/layout/SidebarFooter'
import SidebarNavigation from '@/components/layout/SidebarNavigation'
import { ContentToolbar } from '@/components/TitleBar'
import { LibraryChromeContext } from '@/contexts/library-chrome-context'
import { SidebarSlotContext } from '@/contexts/sidebar-slot-context'
import { useMacTrafficLightPosition } from '@/hooks/useMacTrafficLightPosition'
import { usePlatform } from '@/hooks/usePlatform'
import { useShortcut } from '@/hooks/useShortcut'
import { useWindowDragging } from '@/hooks/useWindowDragging'
import { useWindowFrame } from '@/hooks/useWindowFrame'

interface MainLayoutProps {
  children: ReactNode
  sidebarTitle?: ReactNode
}

interface SidebarAreaProps {
  title?: ReactNode
}

const SidebarArea: React.FC<SidebarAreaProps> = ({ title }) => {
  const selectionId = useId()
  const windowDragging = useWindowDragging()

  return (
    <aside
      data-tauri-drag-region
      {...windowDragging}
      className="flex h-full w-12 shrink-0 flex-col"
    >
      <div data-tauri-drag-region className="h-10 shrink-0">
        {title}
      </div>
      <LayoutGroup id={selectionId}>
        <SidebarNavigation />
        <SidebarFooter />
      </LayoutGroup>
    </aside>
  )
}

/**
 * Linux 系统标题栏布局。
 *
 * When the Linux system frame is enabled, the content uses a flat layout
 * instead of duplicating native window chrome with an inset panel.
 */
interface ContentToolbarProps {
  toolbarHostRef: (element: HTMLDivElement | null) => void
}

const LinuxMainLayout: React.FC<MainLayoutProps & ContentToolbarProps> = ({
  children,
  toolbarHostRef,
}) => {
  return (
    <>
      <SidebarArea />

      <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-card text-card-foreground">
        <div
          data-tauri-drag-region="deep"
          className="flex h-10 shrink-0 items-center justify-end px-3"
        >
          <div ref={toolbarHostRef} className="flex items-center" />
        </div>
        <div className="min-h-0 flex-1">{children}</div>
      </main>
    </>
  )
}

/**
 * 自定义标题栏布局。
 *
 * The app-rendered frame uses one continuous shell background around the
 * sidebar and inset content panel on every desktop platform.
 */
const InsetMainLayout: React.FC<MainLayoutProps & ContentToolbarProps> = ({
  children,
  sidebarTitle,
  toolbarHostRef,
}) => {
  return (
    <>
      <SidebarArea title={sidebarTitle} />

      <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <ContentToolbar
          rightSlot={
            <div ref={toolbarHostRef} className="flex min-w-0 flex-1 items-center justify-end" />
          }
        />
        <div className="flex min-h-0 flex-1 pb-2 pr-2">
          <InsetSurface className="h-full w-full flex-1 rounded-xl">{children}</InsetSurface>
        </div>
      </main>
    </>
  )
}

// The design (Main.dc.html) seats the traffic lights at the top of the
// full-height Library sidebar rather than in a 40pt title bar.
const MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET = { x: 4, y: 8 } as const
// With the sidebar collapsed the lights sit in the top band over the icon
// column, centered on the first content column's 44px header controls.
const MAC_CONTENT_TRAFFIC_LIGHT_OFFSET = { x: 4, y: 16 } as const

/**
 * macOS layout: no title bar row and no icon rail. Each page renders the
 * shared Library sidebar flush with the window edge; it owns the traffic-light
 * strip and the top-level navigation (History, Devices, Settings).
 *
 * The sidebar can be hidden (toolbar toggle, ⌃⌘S), and is hidden in the
 * compact window tier, where the toggle opens it as an overlay drawer instead.
 */
const MacMainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  const compact = useHistoryLayoutTier() === 'compact'
  const [userHidden, setUserHidden] = useState(readLibraryHidden)
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [lightsInContent, setLightsInContent] = useState(false)
  const { pathname } = useLocation()

  // The drawer is a compact-tier overlay; leaving the tier or the page shuts it.
  useEffect(() => setDrawerOpen(false), [compact, pathname])

  const toggle = useCallback(() => {
    if (compact) {
      setDrawerOpen(open => !open)
      return
    }
    setUserHidden(hidden => {
      writeLibraryHidden(!hidden)
      return !hidden
    })
  }, [compact])
  const closeDrawer = useCallback(() => setDrawerOpen(false), [])
  useShortcut({
    id: 'nav.toggleSidebar',
    key: 'meta+ctrl+s',
    scope: 'global',
    handler: toggle,
    enableOnFormTags: true,
  })

  const chrome = useMemo(
    () => ({
      hidden: compact || userHidden,
      drawer: compact,
      drawerOpen: compact && drawerOpen,
      toggle,
      closeDrawer,
      setLightsInContent,
    }),
    [closeDrawer, compact, drawerOpen, toggle, userHidden]
  )

  useMacTrafficLightPosition(
    lightsInContent ? MAC_CONTENT_TRAFFIC_LIGHT_OFFSET : MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET
  )
  return (
    <LibraryChromeContext value={chrome}>
      <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background text-foreground">
        {children}
      </main>
    </LibraryChromeContext>
  )
}

const MAC_SLOT = { contentToolbarHost: null, libraryOwnsNavigation: true } as const

const MainLayout: React.FC<MainLayoutProps> = ({ children, sidebarTitle }) => {
  const { isLinux, isMac, isTauri } = usePlatform()
  const { useSystemWindowFrame } = useWindowFrame()
  const [contentToolbarHost, setContentToolbarHost] = useState<HTMLDivElement | null>(null)
  const railSlot = useMemo(
    () => ({ contentToolbarHost, libraryOwnsNavigation: false }),
    [contentToolbarHost]
  )

  if (isMac) {
    return (
      <SidebarSlotContext value={MAC_SLOT}>
        <MacMainLayout>{children}</MacMainLayout>
      </SidebarSlotContext>
    )
  }

  if (isLinux && isTauri && useSystemWindowFrame) {
    return (
      <SidebarSlotContext value={railSlot}>
        <LinuxMainLayout toolbarHostRef={setContentToolbarHost}>{children}</LinuxMainLayout>
      </SidebarSlotContext>
    )
  }

  return (
    <SidebarSlotContext value={railSlot}>
      <InsetMainLayout sidebarTitle={sidebarTitle} toolbarHostRef={setContentToolbarHost}>
        {children}
      </InsetMainLayout>
    </SidebarSlotContext>
  )
}

export default MainLayout
