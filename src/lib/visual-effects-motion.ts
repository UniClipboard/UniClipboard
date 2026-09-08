import { MotionGlobalConfig, visualElementStore } from 'framer-motion'

/** Apply a live preference without remounting fields, portals or route state. */
export function applyMotionPreference(reduce: boolean) {
  MotionGlobalConfig.skipAnimations = reduce
  for (const element of document.querySelectorAll('*')) {
    const visual = visualElementStore.get(element)
    if (!visual) continue
    visual.shouldReduceMotion = reduce
    visual.shouldSkipAnimations = reduce
    if (reduce) {
      visual.values.forEach(value => value.animation?.complete())
      visual.projection?.finishAnimation()
    }
  }
}
