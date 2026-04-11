import React, { ReactNode } from 'react'
import { Sidebar } from '@/components'
import InsetSurface from '@/components/layout/InsetSurface'

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
  return (
    <>
      {/* Sidebar Navigation */}
      <Sidebar />

      {/* Main Content Area */}
      <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden p-2">
        <InsetSurface className="flex-1 w-full h-full">{children}</InsetSurface>
      </main>
    </>
  )
}

export default MainLayout
