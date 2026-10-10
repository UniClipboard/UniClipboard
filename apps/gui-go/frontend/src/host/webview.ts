// WebView-level controls of the desktop host.
import { Window } from '@wailsio/runtime'

export const getCurrentWebview = () => ({
  setZoom: (scale: number): Promise<void> => Window.SetZoom(scale),
})
