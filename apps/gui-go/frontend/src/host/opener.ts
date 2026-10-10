// Host adapter for `@tauri-apps/plugin-opener`.
import * as HostService from '@host/hostservice'

// Goes through the host so that it can start the browser helper without the AppImage's library environment on Linux (Wails' own call cannot).
export const openUrl = (url: string | URL): Promise<void> => HostService.OpenURL(String(url))
