import { commands } from '@/lib/ipc'
import type { DaemonStartupStatus } from '@/lib/ipc'

let snapshot: DaemonStartupStatus | null = null
let pending: Promise<DaemonStartupStatus | null> | null = null
const listeners = new Set<() => void>()
let timer: ReturnType<typeof setTimeout> | undefined
let generation = 0

export const getStartupSnapshot = () => snapshot
export const startupFailed = (status: DaemonStartupStatus) =>
  status.service_failed || ['failed', 'interrupted'].includes(status.progress.state)

export function refreshStartupSnapshot(): Promise<DaemonStartupStatus | null> {
  if (pending) return pending
  pending = commands
    .getDaemonStartupStatus()
    .then(next => {
      if (
        next &&
        snapshot &&
        next.progress.attempt_id === snapshot.progress.attempt_id &&
        next.progress.sequence < snapshot.progress.sequence
      )
        return snapshot
      snapshot = next
      listeners.forEach(listener => listener())
      return next
    })
    .catch(error => {
      snapshot = null
      listeners.forEach(listener => listener())
      throw error
    })
    .finally(() => {
      pending = null
    })
  return pending
}

async function poll(owner: number) {
  try {
    await refreshStartupSnapshot()
  } catch {
    // IPC reports the error; the connection owner controls transport timeouts.
  } finally {
    if (listeners.size && owner === generation) timer = setTimeout(() => void poll(owner), 500)
  }
}

export function subscribeStartup(listener: () => void) {
  listeners.add(listener)
  if (listeners.size === 1) void poll(++generation)
  return () => {
    listeners.delete(listener)
    if (!listeners.size) {
      generation++
      clearTimeout(timer)
    }
  }
}
