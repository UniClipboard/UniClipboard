// Host adapter for `@tauri-apps/plugin-opener`.
import { invoke } from './core'

// Goes through the host so that it can start the browser helper without the AppImage's library environment on Linux (Wails' own call cannot).
export const openUrl = (url: string | URL): Promise<void> =>
  invoke<null>('open_url', { url: String(url) }).then(() => undefined)
