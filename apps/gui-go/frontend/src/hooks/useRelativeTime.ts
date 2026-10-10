import { useSyncExternalStore } from 'react'
import { useTranslation } from 'react-i18next'

/**
 * One shared clock for the whole app. A single 30s timer broadcasts the
 * current time to every relative-time label, instead of a parent component
 * re-rendering (and recomputing) the entire clipboard list on each tick, or
 * each row owning its own interval. Subscribers re-render only their own
 * timestamp, so idle ticks stay cheap even with a long history list.
 *
 * See issue #1129: the old per-list `setTick` forced an O(N) rebuild of every
 * display item (new object identities) and a full list reconcile every 30s,
 * which janked weak machines even while idle.
 */
const TICK_MS = 30000

let sharedNow = Date.now()
const listeners = new Set<() => void>()
let timer: ReturnType<typeof setInterval> | null = null

function ensureTimer(): void {
  if (timer !== null) return
  timer = setInterval(() => {
    sharedNow = Date.now()
    for (const listener of listeners) listener()
  }, TICK_MS)
}

function subscribe(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange)
  ensureTimer()
  return () => {
    listeners.delete(onStoreChange)
    if (listeners.size === 0 && timer !== null) {
      clearInterval(timer)
      timer = null
    }
  }
}

function getSnapshot(): number {
  return sharedNow
}

/** Current time in ms, refreshed on the shared 30s clock. */
export function useNow(): number {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
}

/** Format an absolute ms timestamp as a relative label against a given `now`. */
export function formatRelativeTime(
  ms: number,
  now: number,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  const diffMins = Math.round((now - ms) / 60000)
  if (diffMins < 1) return t('clipboard.time.justNow')
  if (diffMins < 60) return t('clipboard.time.minutesAgo', { minutes: diffMins })
  if (diffMins < 1440) return t('clipboard.time.hoursAgo', { hours: Math.floor(diffMins / 60) })
  return t('clipboard.time.daysAgo', { days: Math.floor(diffMins / 1440) })
}

/** Live relative-time label for an absolute ms timestamp, ticking every 30s. */
export function useRelativeTime(ms: number): string {
  const now = useNow()
  const { t } = useTranslation()
  return formatRelativeTime(ms, now, t)
}
