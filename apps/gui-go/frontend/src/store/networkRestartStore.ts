import { useSyncExternalStore } from 'react'

// Relay routing has an Engine-owned applied snapshot. The remaining bind-time
// options expose only mutation results, so retain that restart hint in memory.
let restartRequired = false
const saves = new Set<Promise<unknown>>()
const listeners = new Set<() => void>()
const snapshot = () => restartRequired || saves.size > 0
const publish = () => {
  for (const listener of listeners) listener()
}
const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function useNetworkRestartRequired(): boolean {
  return useSyncExternalStore(subscribe, snapshot)
}

/** Include writes flushed by a page that has already unmounted. */
export function trackNetworkSettingsSave<T extends { restartRequired: boolean }>(
  save: Promise<T>
): Promise<T> {
  saves.add(save)
  publish()
  void save.then(
    result => {
      if (!saves.delete(save)) return
      restartRequired ||= result.restartRequired
      publish()
    },
    () => {
      saves.delete(save)
      publish()
    }
  )
  return save
}

export async function awaitNetworkSettingsSaves(): Promise<void> {
  while (saves.size > 0) await Promise.all(saves)
}

export function networkDaemonRestarted(): void {
  restartRequired = false
  publish()
}

/** Reset module state between test cases. */
export function resetNetworkRestartStateForTest(): void {
  saves.clear()
  networkDaemonRestarted()
}
