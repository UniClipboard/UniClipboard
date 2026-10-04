import { LayoutGroup } from 'framer-motion'
import React, { ReactNode, useId } from 'react'
import InsetSurface from '@/components/layout/InsetSurface'
import SidebarFooter from '@/components/layout/SidebarFooter'
import SidebarNavigation from '@/components/layout/SidebarNavigation'
import { ContentToolbar } from '@/components/TitleBar'
import { useMacTrafficLightPosition } from '@/hooks/useMacTrafficLightPosition'
import { usePlatform } from '@/hooks/usePlatform'
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
const LinuxMainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  return (
    <>
      <SidebarArea />

      <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-card text-card-foreground">
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
const InsetMainLayout: React.FC<MainLayoutProps> = ({ children, sidebarTitle }) => {
  return (
    <>
      <SidebarArea title={sidebarTitle} />

      <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <ContentToolbar />
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

/**
 * macOS layout: no title bar row and no icon rail. Each page renders the
 * shared Library sidebar flush with the window edge; it owns the traffic-light
 * strip and the top-level navigation (History, Devices, Settings).
 */
const MacMainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  useMacTrafficLightPosition(MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET)
  return (
    <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background text-foreground">
      {children}
    </main>
  )
}

const MainLayout: React.FC<MainLayoutProps> = ({ children, sidebarTitle }) => {
  const { isLinux, isMac, isTauri } = usePlatform()
  const { useSystemWindowFrame } = useWindowFrame()
  if (isMac) {
    return <MacMainLayout>{children}</MacMainLayout>
  }

  if (isLinux && isTauri && useSystemWindowFrame) {
    return <LinuxMainLayout>{children}</LinuxMainLayout>
  }

  return <InsetMainLayout sidebarTitle={sidebarTitle}>{children}</InsetMainLayout>
}

export default MainLayout
