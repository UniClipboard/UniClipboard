import React, { ReactNode } from 'react'
import { Sidebar } from '@/components'
import { usePlatform } from '@/hooks/usePlatform'

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
      <Sidebar className={isWindows ? '-mt-10 pt-14' : undefined} />

      {/* Main Content Area */}
      <main className="relative flex-1 flex flex-col overflow-hidden">
        {isWindows ? (
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-x-0 top-0 z-0 h-16 bg-[linear-gradient(180deg,color-mix(in_oklab,var(--background)_74%,transparent),transparent)]"
          />
        ) : null}
        <div className="relative z-10 flex-1 overflow-hidden">{children}</div>
      </main>
    </>
  )
}

export default MainLayout
