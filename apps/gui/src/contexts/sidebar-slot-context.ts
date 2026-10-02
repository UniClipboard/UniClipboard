import { createContext, use } from 'react'

interface SidebarSlotContextType {
  contentToolbarHost: HTMLElement | null
}

export const SidebarSlotContext = createContext<SidebarSlotContextType | undefined>(undefined)

export function useSidebarSlot() {
  const context = use(SidebarSlotContext)
  if (!context) throw new Error('useSidebarSlot must be used within SidebarSlotContext')
  return context
}
