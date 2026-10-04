import { createContext, use } from 'react'

/**
 * Show/hide state of the macOS Library sidebar and where the window's traffic
 * lights sit. Provided by the macOS layout; the default (sidebar shown, nothing
 * to toggle) covers Windows, Linux and tests.
 */
export interface LibraryChrome {
  /** The sidebar is hidden: the compact window tier, or the user hid it. It
   * then collapses to an icon column under a top band that the traffic lights
   * share with the first content column's header. */
  hidden: boolean
  /** Hidden because the window is compact: the toggle opens an overlay drawer
   * instead of showing the sidebar inline. */
  drawer: boolean
  drawerOpen: boolean
  /** Toolbar button and ⌃⌘S: inline show/hide, or open/close the drawer. */
  toggle: () => void
  closeDrawer: () => void
  /** Pages with the Library sidebar report whether the lights sit in the
   * collapsed sidebar's top band (centered on the content header) or over the
   * expanded sidebar. */
  setLightsInContent: (inContent: boolean) => void
}

const noop = () => {}

export const LibraryChromeContext = createContext<LibraryChrome>({
  hidden: false,
  drawer: false,
  drawerOpen: false,
  toggle: noop,
  closeDrawer: noop,
  setLightsInContent: noop,
})

export function useLibraryChrome(): LibraryChrome {
  return use(LibraryChromeContext)
}
