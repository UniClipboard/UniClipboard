// WebView-level controls of the desktop host.
import { commands } from '@/lib/ipc'

export const getCurrentWebview = () => ({
  setZoom: (scale: number): Promise<void> => commands.setWindowZoom(scale),
})
