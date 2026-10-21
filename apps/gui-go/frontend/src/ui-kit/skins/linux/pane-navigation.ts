import { createContext, use } from 'react'

export const PaneNavigation = createContext<{
  single: boolean
  detail: boolean
} | null>(null)
export const usePaneNavigation = () => use(PaneNavigation)
