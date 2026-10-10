import { createContext, use } from 'react'

export interface TitleBarSlotContextType {
  rightSlotHost: HTMLElement | null
}

export const TitleBarSlotContext = createContext<TitleBarSlotContextType | undefined>(undefined)

export function useTitleBarSlot() {
  const ctx = use(TitleBarSlotContext)
  if (!ctx) throw new Error('useTitleBarSlot must be used within TitleBarSlotProvider')
  return ctx
}
