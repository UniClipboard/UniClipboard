// Host adapter for `@tauri-apps/api/core`: routes `invoke` to the Go host.
import { Call, Events } from '@wailsio/runtime'

interface InvokeResult {
  ok: boolean
  data?: unknown
  error?: unknown
}

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const encoded: Record<string, unknown> = {}
  const unsubscribe: Array<() => void> = []
  for (const [key, value] of Object.entries(args)) {
    if (value === undefined) continue
    if (value instanceof Channel) {
      // Channels cross the bridge as an id; the host delivers messages as events on it.
      encoded[key] = { __channel: value.id }
      unsubscribe.push(Events.On(`channel://${value.id}`, event => value.onmessage(event.data)))
    } else {
      encoded[key] = value
    }
  }
  try {
    const result = (await Call.ByName('main.HostService.Invoke', command, encoded)) as InvokeResult
    if (!result.ok) throw result.error ?? new Error(`command ${command} failed`)
    return (result.data ?? null) as T
  } finally {
    for (const off of unsubscribe) off()
  }
}

// The shared frontend uses this flag for "running inside the desktop shell".
export const isTauri = (): boolean => true

let channelSequence = 0

// Stream of host messages for one command invocation (e.g. install progress).
export class Channel<T> {
  readonly id = `${Date.now().toString(36)}-${++channelSequence}`
  onmessage: (message: T) => void = () => undefined
}

export const convertFileSrc = (path: string): string =>
  `/host-file?path=${encodeURIComponent(path)}`
