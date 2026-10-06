// Host adapter for `@tauri-apps/api/core`: routes `invoke` to the Go host.
import { Call } from '@wailsio/runtime'

interface InvokeResult {
  ok: boolean
  data?: unknown
  error?: unknown
}

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const encoded: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(args)) {
    if (value !== undefined) encoded[key] = value
  }
  const result = (await Call.ByName('main.HostService.Invoke', command, encoded)) as InvokeResult
  if (!result.ok) throw result.error ?? new Error(`command ${command} failed`)
  return (result.data ?? null) as T
}

// The shared frontend uses this flag for "running inside the desktop shell".
export const isTauri = (): boolean => true

export class Channel<T> {
  onmessage: (message: T) => void = () => undefined
}

export const convertFileSrc = (path: string): string =>
  `/host-file?path=${encodeURIComponent(path)}`
