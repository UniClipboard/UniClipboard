import { type ReactNode } from 'react'
import { Outlet } from 'react-router'
import { WindowControls } from '@/components/WindowControls'
import { useWindowDragging } from '@/hooks/useWindowDragging'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { WINDOW_CONTROLS_INSET_STYLE } from '@/lib/window-controls-inset'

/**
 * Layout for the full-screen settings page.
 * This layout does not include the main sidebar and has a simpler structure.
 *
 * Note: This is rendered within WindowShell, so no need for h-screen wrapper.
 * WindowShell already provides the height constraint via flex-col structure.
 *
 * A page that draws the window top itself passes no `titleBar`. With an app-drawn frame the window
 * controls then float over its top-right corner, as in `LibraryMainLayout`.
 */
const SettingsFullLayout = ({ titleBar }: { titleBar?: ReactNode }) => {
  const { hasCustomWindowControls } = useWindowFrame()
  const windowDragging = useWindowDragging()
  const floatingControls = !titleBar && hasCustomWindowControls
  return (
    <div
      className="relative w-full h-full flex flex-col"
      style={floatingControls ? WINDOW_CONTROLS_INSET_STYLE : undefined}
    >
      {titleBar}
      <div className="min-h-0 flex-1">
        <Outlet />
      </div>
      {floatingControls && (
        <div
          data-tauri-drag-region
          {...windowDragging}
          className="absolute right-0 top-0 z-50 h-12 select-none"
        >
          <WindowControls />
        </div>
      )}
    </div>
  )
}

export default SettingsFullLayout
