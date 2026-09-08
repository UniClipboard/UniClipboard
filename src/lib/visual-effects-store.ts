import { visualEffectsApi } from '@/api/visual-effects'
import type { EffectsMode, EffectsSnapshot, SystemMotion } from '@/lib/ipc-bindings.generated'
import { applyMotionPreference } from '@/lib/visual-effects-motion'

export const INITIAL_EFFECTS: EffectsSnapshot = {
  sessionId: '',
  revision: 0,
  mode: 'auto',
  autoForSession: 'smooth',
  nextAuto: null,
  systemMotion: 'unknown',
  reduceMotion: true,
  lowEffects: true,
  reason: 'unknown',
  persistence: 'session_only',
}

export function createVisualEffectsStore(
  api: typeof visualEffectsApi,
  apply: (value: EffectsSnapshot) => void
) {
  let snapshot = INITIAL_EFFECTS
  let unavailable = false
  const listeners = new Set<() => void>()
  const emit = () => listeners.forEach(listener => listener())
  const accept = (next: EffectsSnapshot) => {
    if (snapshot.sessionId && next.sessionId !== snapshot.sessionId) return
    if (next.revision < snapshot.revision) return
    if (JSON.stringify(next) === JSON.stringify(snapshot) && !unavailable) return
    snapshot = next
    unavailable = false
    apply(next)
    emit()
  }
  const fail = () => {
    unavailable = true
    emit()
  }
  return {
    getSnapshot: () => snapshot,
    isUnavailable: () => unavailable,
    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    accept,
    async refresh() {
      try {
        accept(await api.get())
      } catch {
        fail()
      }
    },
    async environment(motion: SystemMotion) {
      if (!snapshot.sessionId) return
      try {
        accept(await api.environment(snapshot.sessionId, motion))
      } catch {
        fail()
      }
    },
    async setMode(mode: EffectsMode) {
      try {
        accept(await api.setMode(mode))
      } catch (error) {
        fail()
        throw error
      }
    },
  }
}

function apply(snapshot: EffectsSnapshot) {
  const root = document.documentElement
  const motionChanged = root.dataset.ucReduceMotion !== String(snapshot.reduceMotion)
  root.dataset.ucLowEffects = String(snapshot.lowEffects)
  root.dataset.ucReduceMotion = String(snapshot.reduceMotion)
  if (motionChanged) applyMotionPreference(snapshot.reduceMotion)
}

export const visualEffectsStore = createVisualEffectsStore(visualEffectsApi, apply)
export const isMotionReduced = () => visualEffectsStore.getSnapshot().reduceMotion

export function initializeVisualEffects(): () => void {
  apply(INITIAL_EFFECTS)
  let active = true
  let disposeEvents: (() => void) | undefined
  let syncing = false
  let syncAgain = false
  let media: MediaQueryList | undefined
  try {
    media = window.matchMedia('(prefers-reduced-motion: reduce)')
  } catch {
    /* Unknown stays conservative. */
  }
  const systemMotion = (): SystemMotion =>
    media ? (media.matches ? 'reduce' : 'allow') : 'unknown'
  const sync = async () => {
    if (syncing) {
      syncAgain = true
      return
    }
    syncing = true
    do {
      syncAgain = false
      if (!disposeEvents) {
        try {
          const dispose = await visualEffectsApi.subscribe(next => {
            if (active) visualEffectsStore.accept(next)
          })
          if (!active) {
            dispose()
            break
          }
          disposeEvents = dispose
        } catch {
          /* A snapshot still works when notification registration fails. */
        }
      }
      if (!active) break
      await visualEffectsStore.refresh()
      if (active) await visualEffectsStore.environment(systemMotion())
    } while (active && syncAgain)
    syncing = false
  }
  const onVisible = () => {
    if (document.visibilityState === 'visible') void sync()
  }
  const onChange = () => {
    void sync()
  }
  media?.addEventListener('change', onChange)
  document.addEventListener('visibilitychange', onVisible)
  window.addEventListener('focus', onVisible)
  void sync()
  return () => {
    active = false
    disposeEvents?.()
    media?.removeEventListener('change', onChange)
    document.removeEventListener('visibilitychange', onVisible)
    window.removeEventListener('focus', onVisible)
  }
}
