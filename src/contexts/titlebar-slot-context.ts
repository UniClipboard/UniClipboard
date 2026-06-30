import { createContext, use, type ReactNode } from 'react'

export interface TitleBarSlotContextType {
  rightSlot: ReactNode
  setRightSlot: (node: ReactNode) => void
}

export const TitleBarSlotContext = createContext<TitleBarSlotContextType | undefined>(undefined)

export function useTitleBarSlot() {
  const ctx = use(TitleBarSlotContext)
  if (!ctx) throw new Error('useTitleBarSlot must be used within TitleBarSlotProvider')
  return ctx
}
