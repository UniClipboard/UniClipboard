// Host adapter for `@tauri-apps/api/event` on top of the Wails event bus.
import { Events } from '@wailsio/runtime'

export interface Event<T> {
  event: string
  id: number
  payload: T
}
export type UnlistenFn = () => void
export type EventCallback<T> = (event: Event<T>) => void

let sequence = 0

export async function listen<T>(name: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  return Events.On(name, wails =>
    handler({ event: name, id: ++sequence, payload: wails.data as T })
  )
}

export async function emit(name: string, payload?: unknown): Promise<void> {
  await Events.Emit(name, payload)
}
