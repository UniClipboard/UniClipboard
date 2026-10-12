import { createContext, use } from 'react'

/** Popups portal into the themed surface that owns them, so a dark panel's menu keeps dark tokens. */
export const PortalContainer = createContext<HTMLElement | null>(null)

export function usePortalContainer() {
  return use(PortalContainer)
}
