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
    <div className="relative h-screen flex flex-col overflow-hidden bg-background text-foreground transition-colors duration-200">
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_top_left,rgba(255,255,255,0.14),transparent_34%),linear-gradient(180deg,color-mix(in_oklab,var(--background)_98%,transparent),color-mix(in_oklab,var(--background)_92%,transparent)_22%,var(--background)_100%)] dark:bg-[radial-gradient(circle_at_top_left,rgba(255,255,255,0.08),transparent_32%),linear-gradient(180deg,color-mix(in_oklab,var(--background)_96%,transparent),color-mix(in_oklab,var(--background)_90%,transparent)_22%,var(--background)_100%)]"
      />
      {/* Window Chrome Layer - Full width titlebar */}
      {titleBar}

      {/* Content Area Layer - Sidebar + Main */}
      <div className="relative z-10 flex-1 flex overflow-hidden">{children}</div>
    </div>
  )
}

export default WindowShell
