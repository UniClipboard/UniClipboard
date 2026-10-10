// Page-side event bus of the desktop host, on top of the Wails event bus.
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

export async function emit<T>(name: string, payload?: T): Promise<void> {
  await Events.Emit(name, payload)
}
