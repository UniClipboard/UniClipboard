import { useSyncExternalStore } from 'react'

function subscribe(onChange: () => void) {
  const observer = new MutationObserver(onChange)
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
  return () => observer.disconnect()
}

/**
 * The colour scheme the window theme controller applied (`lib/window-theme.ts` puts `light` or
 * `dark` on the root element), so pages follow the app theme setting, the system appearance and the
 * desktop theme from one source.
 */
export function useAppliedColorScheme(): 'light' | 'dark' {
  return useSyncExternalStore(
    subscribe,
    () => (document.documentElement.classList.contains('dark') ? 'dark' : 'light'),
    () => 'light'
  )
}
