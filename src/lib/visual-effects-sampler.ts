import { visualEffectsApi } from '@/api/visual-effects'
import { visualEffectsStore } from '@/lib/visual-effects-store'

export function summarizeFrames(intervals: number[]) {
  if (
    intervals.length < 30 ||
    intervals.some(value => !Number.isFinite(value) || value <= 0 || value > 500)
  )
    return null
  const durationMs = intervals.reduce((sum, value) => sum + value, 0)
  if (durationMs > 2000) return null
  return {
    frames: intervals.length,
    durationMs,
    longFrames: intervals.filter(value => value > 50).length,
    longestMs: Math.max(...intervals),
  }
}

/** One bounded sample after a real foreground interaction; no idle timer. */
export function startVisualEffectsSampler(isReady: () => boolean): () => void {
  let active = true
  let raf = 0
  let waiting = false
  let generation = 0
  let nextEligible = performance.now() + 10_000
  const cancel = () => {
    generation += 1
    cancelAnimationFrame(raf)
    raf = 0
    nextEligible = Math.max(nextEligible, performance.now() + 2000)
  }
  const interaction = async (event: Event) => {
    const state = visualEffectsStore.getSnapshot()
    if (
      !event.isTrusted ||
      waiting ||
      raf ||
      !active ||
      !isReady() ||
      state.mode !== 'auto' ||
      state.reduceMotion ||
      !state.sessionId ||
      document.visibilityState !== 'visible' ||
      performance.now() < nextEligible
    )
      return
    const epoch = generation
    waiting = true
    nextEligible = performance.now() + 30_000
    try {
      const permit = await visualEffectsApi.beginSample(state.sessionId)
      if (!active || epoch !== generation || !permit) return
      if (!isReady() || document.visibilityState !== 'visible') return
      const start = performance.now()
      let previous = start
      const intervals: number[] = []
      const frame = (now: number) => {
        raf = 0
        const current = visualEffectsStore.getSnapshot()
        if (
          !active ||
          epoch !== generation ||
          !isReady() ||
          document.visibilityState !== 'visible' ||
          current.revision !== permit.revision ||
          current.sessionId !== permit.sessionId
        )
          return
        if (now - previous > 500) return
        if (now - start >= 1950) {
          const summary = summarizeFrames(intervals)
          if (summary)
            void visualEffectsApi
              .reportSample({ ...permit, ...summary })
              .then(next => {
                if (active) visualEffectsStore.accept(next)
              })
              .catch(() => {})
          return
        }
        intervals.push(now - previous)
        previous = now
        raf = requestAnimationFrame(frame)
      }
      raf = requestAnimationFrame(frame)
    } catch {
      /* A refused sample never blocks the user's operation. */
    } finally {
      waiting = false
    }
  }
  const onInteraction = (event: Event) => {
    void interaction(event)
  }
  const onVisibility = () => {
    cancel()
  }
  const unsubscribe = visualEffectsStore.subscribe(cancel)
  for (const type of ['pointerdown', 'keydown', 'wheel'])
    document.addEventListener(type, onInteraction, { capture: true, passive: true })
  document.addEventListener('visibilitychange', onVisibility)
  window.addEventListener('pagehide', onVisibility)
  return () => {
    active = false
    cancel()
    unsubscribe()
    for (const type of ['pointerdown', 'keydown', 'wheel'])
      document.removeEventListener(type, onInteraction, true)
    document.removeEventListener('visibilitychange', onVisibility)
    window.removeEventListener('pagehide', onVisibility)
  }
}
