import { useSyncExternalStore } from 'react'

// Below this width a readable list and detail no longer fit beside each other.
const singlePaneQuery = '(max-width: 760px)'
const subscribe = (notify: () => void) => {
  const media = window.matchMedia(singlePaneQuery)
  media.addEventListener('change', notify)
  return () => media.removeEventListener('change', notify)
}
const snapshot = () => window.matchMedia(singlePaneQuery).matches

export function useSinglePaneLayout() {
  return useSyncExternalStore(subscribe, snapshot, () => false)
}
