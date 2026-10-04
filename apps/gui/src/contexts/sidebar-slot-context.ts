import { createContext, use } from 'react'

/** What the main layout gives its pages; platform differences stop here. */
interface SidebarSlotContextType {
  /** Toolbar element pages portal their search into; null when the layout has
   * no toolbar (macOS), where pages put search in their list column instead. */
  contentToolbarHost: HTMLElement | null
  /** True when the shared Library sidebar is the window's top-level navigation
   * (macOS); false when the icon rail is (Windows, Linux). */
  libraryOwnsNavigation: boolean
}

export const SidebarSlotContext = createContext<SidebarSlotContextType | undefined>(undefined)

export function useSidebarSlot() {
  const context = use(SidebarSlotContext)
  if (!context) throw new Error('useSidebarSlot must be used within SidebarSlotContext')
  return context
}
