import { createContext, useContext } from 'react'

/**
 * How SplitView shows its sidebar right now (WinUI NavigationView display modes): `expanded` (Left, or the pane
 * opened over the content) or `compact` (the 48 px icon rail, LeftCompact). The sidebar's SidebarNav reads it.
 */
export type SidebarDisplay = 'expanded' | 'compact'

const DisplayContext = createContext<SidebarDisplay>('expanded')

export const SidebarDisplayProvider = DisplayContext.Provider
export const useSidebarDisplay = () => useContext(DisplayContext)
