// Host adapter for `@tauri-apps/api/webview`.
import { Window } from '@wailsio/runtime'

export const getCurrentWebview = () => ({
  setZoom: (scale: number): Promise<void> => Window.SetZoom(scale),
})
