import { createContext, use } from 'react'

/**
 * Element the app's own popups (dialogs, popovers, select lists, tooltips) portal into instead of
 * `<body>`. A themed page provides one so those popups keep its tokens; without a provider they
 * portal to `<body>` as before.
 */
export const PortalScope = createContext<HTMLElement | null>(null)

export function usePortalScope(): HTMLElement | undefined {
  return use(PortalScope) ?? undefined
}
