import { MotionGlobalConfig, visualElementStore } from 'framer-motion'

/** Apply a live preference without remounting fields, portals or route state. */
export function applyMotionPreference(reduce: boolean) {
  MotionGlobalConfig.skipAnimations = reduce
  // Fluent's Web Animations are independent of Motion and CSS transitions.
  // Complete finite UI animations so exit callbacks and focus restoration run.
  if (reduce && typeof document.getAnimations === 'function') {
    for (const animation of document.getAnimations()) {
      const target = (animation.effect as KeyframeEffect | null)?.target
      if (!(target instanceof Element) || !target.closest('.fui-FluentProvider')) continue
      const end = animation.effect?.getComputedTiming().endTime
      if (typeof end !== 'number' || !Number.isFinite(end)) continue
      if (animation.playbackRate === 0) animation.playbackRate = 1
      animation.finish()
    }
  }
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
