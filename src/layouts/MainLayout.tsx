import React, { ReactNode } from 'react'
import { Sidebar } from '@/components'
import { usePlatform } from '@/hooks/usePlatform'
import { cn } from '@/lib/utils'

interface MainLayoutProps {
  children: ReactNode
}

/**
 * Main content layout with sidebar navigation
 *
 * Structure (within WindowShell):
 * - Sidebar: Fixed-width navigation (w-16)
 * - Main: Flexible content area (flex-1)
 *
 * Note: This is a content-level layout, not window-level.
 * Window chrome (TitleBar) is handled by WindowShell parent.
 */
const MainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  const { isWindows } = usePlatform()

  return (
    <>
      {/* Sidebar Navigation */}
      <Sidebar />

      {/* Main Content Area */}
      <main
        className={cn(
          'relative flex-1 flex flex-col overflow-hidden',
          isWindows &&
            'rounded-tl-[22px] bg-background/92 shadow-[inset_1px_1px_0_rgba(255,255,255,0.2)] ring-1 ring-border/35 backdrop-blur-sm'
        )}
      >
        {children}
      </main>
    </>
  )
}

export default MainLayout
