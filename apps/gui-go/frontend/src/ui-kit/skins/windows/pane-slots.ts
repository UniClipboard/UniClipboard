import { createContext, useContext } from 'react'

/**
 * Where a pane's CommandBar goes: the element at the top of the pane (board WA2). An ActionBar inside the pane
 * renders there through a portal, so its place in the DOM, and so the Tab order, matches where it is drawn.
 */
export interface PaneSlots {
  commandBar: HTMLElement | null
  status: HTMLElement | null
}

const SlotsContext = createContext<PaneSlots | null>(null)

export const PaneSlotsProvider = SlotsContext.Provider
export const usePaneSlots = () => useContext(SlotsContext)
