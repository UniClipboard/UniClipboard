import { useSyncExternalStore } from 'react'
import { visualEffectsStore } from '@/lib/visual-effects-store'

export function useVisualEffects() {
  return useSyncExternalStore(
    visualEffectsStore.subscribe,
    visualEffectsStore.getSnapshot,
    visualEffectsStore.getSnapshot
  )
}

export function useReducedMotion() {
  return useVisualEffects().reduceMotion
}

export function useVisualEffectsUnavailable() {
  return useSyncExternalStore(
    visualEffectsStore.subscribe,
    visualEffectsStore.isUnavailable,
    visualEffectsStore.isUnavailable
  )
}
