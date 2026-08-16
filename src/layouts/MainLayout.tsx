import React, { ReactNode } from 'react'
import { Sidebar } from '@/components'
import InsetSurface from '@/components/layout/InsetSurface'
import { usePlatform } from '@/hooks/usePlatform'
import { useWindowFrame } from '@/hooks/useWindowFrame'

interface MainLayoutProps {
  children: ReactNode
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
      <Sidebar className="border-r border-border/40 bg-background/80 dark:bg-background/60" />

      <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-card text-card-foreground">
        {children}
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
const InsetMainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  return (
    <>
      <Sidebar />

      <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden pb-2 pr-2">
        <InsetSurface className="flex-1 w-full h-full">{children}</InsetSurface>
      </main>
    </>
  )
}

const MainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  const { isLinux, isTauri } = usePlatform()
  const { useSystemWindowFrame } = useWindowFrame()

  if (isLinux && isTauri && useSystemWindowFrame) {
    return <LinuxMainLayout>{children}</LinuxMainLayout>
  }

  return <InsetMainLayout>{children}</InsetMainLayout>
}

export default MainLayout
