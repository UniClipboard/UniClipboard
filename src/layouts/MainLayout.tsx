import { getCurrentWindow } from '@tauri-apps/api/window'
import { m } from 'framer-motion'
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import React, { ReactNode, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import DeviceManagerDialog from '@/components/device/DeviceManagerDialog'
import InsetSurface from '@/components/layout/InsetSurface'
import SidebarFooter from '@/components/layout/SidebarFooter'
import { SidebarSlotContext } from '@/contexts/sidebar-slot-context'
import { useTitleBarSlot } from '@/contexts/titlebar-slot-context'
import { usePlatform } from '@/hooks/usePlatform'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { cn } from '@/lib/utils'

interface MainLayoutProps {
  children: ReactNode
  sidebarTitle?: ReactNode
}

interface SidebarAreaProps {
  collapsed: boolean
  hostRef: (element: HTMLDivElement | null) => void
  onOpenDevices: () => void
  title?: ReactNode
}

interface ContentToolbarProps {
  toolbarHostRef: (element: HTMLDivElement | null) => void
}

const SidebarArea: React.FC<SidebarAreaProps> = ({ collapsed, hostRef, onOpenDevices, title }) => {
  const dragStartRef = useRef<{ x: number; y: number } | null>(null)

  const handlePointerDown = (event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return
    dragStartRef.current = { x: event.clientX, y: event.clientY }
  }

  const handlePointerMove = (event: React.PointerEvent<HTMLElement>) => {
    const start = dragStartRef.current
    if (!start || (event.buttons & 1) === 0) return
    if (Math.hypot(event.clientX - start.x, event.clientY - start.y) < 4) return
    dragStartRef.current = null
    void getCurrentWindow()
      .startDragging()
      .catch(() => undefined)
  }

  return (
    <aside
      data-tauri-drag-region
      onPointerDownCapture={handlePointerDown}
      onPointerMoveCapture={handlePointerMove}
      onPointerUpCapture={() => {
        dragStartRef.current = null
      }}
      className={cn(
        'flex h-full shrink-0 flex-col transition-[width] duration-200',
        collapsed ? 'w-12' : 'w-48'
      )}
    >
      {title}
      <div data-tauri-drag-region ref={hostRef} className="min-h-0 flex-1 overflow-hidden" />
      <SidebarFooter collapsed={collapsed} onOpenDevices={onOpenDevices} />
    </aside>
  )
}

/**
 * Linux 系统标题栏布局。
 *
 * When the Linux system frame is enabled, the content uses a flat layout
 * instead of duplicating native window chrome with an inset panel.
 */
const LinuxMainLayout: React.FC<MainLayoutProps & SidebarAreaProps & ContentToolbarProps> = ({
  children,
  collapsed,
  hostRef,
  onOpenDevices,
  toolbarHostRef,
}) => {
  return (
    <>
      <SidebarArea collapsed={collapsed} hostRef={hostRef} onOpenDevices={onOpenDevices} />

      <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-card text-card-foreground">
        <div data-tauri-drag-region className="flex h-10 shrink-0 items-center justify-end px-3">
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
const InsetMainLayout: React.FC<MainLayoutProps & SidebarAreaProps & ContentToolbarProps> = ({
  children,
  collapsed,
  hostRef,
  onOpenDevices,
  sidebarTitle,
  toolbarHostRef,
}) => {
  return (
    <>
      <SidebarArea
        collapsed={collapsed}
        hostRef={hostRef}
        onOpenDevices={onOpenDevices}
        title={sidebarTitle}
      />

      <main className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <div data-tauri-drag-region className="flex h-10 shrink-0 items-center justify-end px-3">
          <div ref={toolbarHostRef} className="flex items-center" />
        </div>
        <div className="flex min-h-0 flex-1 pb-2 pr-2">
          <InsetSurface className="h-full w-full flex-1 rounded-xl">{children}</InsetSurface>
        </div>
      </main>
    </>
  )
}

const MainLayout: React.FC<MainLayoutProps> = ({ children, sidebarTitle }) => {
  const { t } = useTranslation()
  const { rightSlotHost } = useTitleBarSlot()
  const { isLinux, isTauri } = usePlatform()
  const { useSystemWindowFrame } = useWindowFrame()
  const [sidebarHost, setSidebarHost] = useState<HTMLDivElement | null>(null)
  const [contentToolbarHost, setContentToolbarHost] = useState<HTMLDivElement | null>(null)
  const [deviceManagerOpen, setDeviceManagerOpen] = useState(false)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)
  const sidebarSlot = useMemo(
    () => ({ sidebarCollapsed, sidebarHost, contentToolbarHost }),
    [contentToolbarHost, sidebarCollapsed, sidebarHost]
  )
  const sidebarToggle = (
    <m.button
      data-tauri-drag-region="false"
      layoutId="main-sidebar-toggle"
      type="button"
      aria-label={t(sidebarCollapsed ? 'nav.expandSidebar' : 'nav.collapseSidebar')}
      title={t(sidebarCollapsed ? 'nav.expandSidebar' : 'nav.collapseSidebar')}
      onClick={() => setSidebarCollapsed(collapsed => !collapsed)}
      className="flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
      transition={{ type: 'spring', stiffness: 420, damping: 34 }}
    >
      {sidebarCollapsed ? (
        <PanelLeftOpen className="size-4" />
      ) : (
        <PanelLeftClose className="size-4" />
      )}
    </m.button>
  )

  if (isLinux && isTauri && useSystemWindowFrame) {
    return (
      <SidebarSlotContext value={sidebarSlot}>
        {rightSlotHost ? createPortal(sidebarToggle, rightSlotHost) : null}
        <LinuxMainLayout
          collapsed={false}
          hostRef={setSidebarHost}
          onOpenDevices={() => setDeviceManagerOpen(true)}
          toolbarHostRef={setContentToolbarHost}
        >
          {children}
        </LinuxMainLayout>
        <DeviceManagerDialog open={deviceManagerOpen} onOpenChange={setDeviceManagerOpen} />
      </SidebarSlotContext>
    )
  }

  return (
    <SidebarSlotContext value={sidebarSlot}>
      {rightSlotHost ? createPortal(sidebarToggle, rightSlotHost) : null}
      <InsetMainLayout
        collapsed={sidebarCollapsed}
        hostRef={setSidebarHost}
        onOpenDevices={() => setDeviceManagerOpen(true)}
        sidebarTitle={sidebarTitle}
        toolbarHostRef={setContentToolbarHost}
      >
        {children}
      </InsetMainLayout>
      <DeviceManagerDialog open={deviceManagerOpen} onOpenChange={setDeviceManagerOpen} />
    </SidebarSlotContext>
  )
}

export default MainLayout
