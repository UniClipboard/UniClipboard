import React, { ReactNode, useCallback, useEffect, useMemo, useState } from 'react'
import { useLocation } from 'react-router'
import {
  readLibraryHidden,
  useHistoryLayoutTier,
  writeLibraryHidden,
} from '@/components/history/layout/history-layout'
import { WindowControls } from '@/components/WindowControls'
import { LibraryChromeContext } from '@/contexts/library-chrome-context'
import { useMacTrafficLightPosition } from '@/hooks/useMacTrafficLightPosition'
import { useShortcut } from '@/hooks/useShortcut'
import { useWindowDragging } from '@/hooks/useWindowDragging'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { WINDOW_CONTROLS_INSET_STYLE } from '@/lib/window-controls-inset'

interface LibraryMainLayoutProps {
  children: ReactNode
}

// The design (Main.dc.html) seats the traffic lights at the top of the
// full-height Library sidebar rather than in a 40pt title bar.
const MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET = { x: 4, y: 8 } as const
// With the sidebar collapsed the lights sit in the top band over the icon
// column, centered on the first content column's 44px header controls.
const MAC_CONTENT_TRAFFIC_LIGHT_OFFSET = { x: 4, y: 16 } as const

/**
 * The main window layout on every platform: no title bar row and no icon
 * rail. Each page renders the shared Library sidebar flush with the window
 * edge; it owns the top-level navigation (History, Devices, Settings).
 *
 * Platform differences are limited to the window chrome: on macOS the sidebar
 * reserves the traffic-light strip; with an app-drawn frame on Windows/Linux
 * the window controls float over the top-right corner (the OS draws its own
 * title bar above the layout when the system frame is selected).
 *
 * The sidebar can be hidden (toolbar toggle, ⌃⌘S), and is hidden in the
 * compact window tier, where the toggle opens it as an overlay drawer instead.
 */
const LibraryMainLayout: React.FC<LibraryMainLayoutProps> = ({ children }) => {
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

  // No-op outside macOS.
  useMacTrafficLightPosition(
    lightsInContent ? MAC_CONTENT_TRAFFIC_LIGHT_OFFSET : MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET
  )
  const { hasCustomWindowControls } = useWindowFrame()
  const windowDragging = useWindowDragging()
  return (
    <LibraryChromeContext value={chrome}>
      <main
        // Pages inset their top-right headers by this much so they clear the
        // window controls overlay (see WINDOW_CONTROLS_* in window-controls-inset).
        style={hasCustomWindowControls ? WINDOW_CONTROLS_INSET_STYLE : undefined}
        className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background text-foreground"
      >
        {children}
        {hasCustomWindowControls && (
          <div
            data-tauri-drag-region
            {...windowDragging}
            className="absolute right-0 top-0 z-50 h-10 select-none"
          >
            <WindowControls />
          </div>
        )}
      </main>
    </LibraryChromeContext>
  )
}

export default LibraryMainLayout
