import React, { ReactNode } from 'react'

interface WindowShellProps {
  titleBar: ReactNode
  children: ReactNode
}

/**
 * Window-level container for Tauri app
 *
 * Architecture:
 * - Titlebar (window chrome layer): Full-width drag region with traffic lights
 * - Content Area (app layout layer): Sidebar + Main content
 *
 * This structure ensures:
 * 1. Titlebar spans entire window width (not affected by Sidebar)
 * 2. macOS traffic lights always positioned at top-left corner
 * 3. Proper z-index layering without manual z-index hacks
 * 4. Content area (Sidebar + Main) sits below titlebar in document flow
 */
export const WindowShell: React.FC<WindowShellProps> = ({ titleBar, children }) => {
  return (
    <div className="relative h-screen flex flex-col overflow-hidden bg-[#F2F2F7] dark:bg-[#09090B] text-foreground transition-colors duration-500">
      {/* Dynamic Background Accents */}
      <div
        data-uc-decorative-effect="true"
        className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_50%_0%,var(--primary)_0%,transparent_100%)] opacity-[0.04] dark:opacity-[0.05]"
        aria-hidden="true"
      />
      <div
        data-uc-decorative-effect="true"
        className="pointer-events-none absolute -left-20 -top-20 size-64 rounded-full bg-primary/5 blur-[100px] dark:bg-primary/10"
        aria-hidden="true"
      />

      {/* Window Chrome Layer - Full width titlebar */}
      {titleBar}

      {/* Content Area Layer - Sidebar + Main */}
      <div className="relative z-10 flex-1 flex overflow-hidden">{children}</div>
    </div>
  )
}

export default WindowShell
