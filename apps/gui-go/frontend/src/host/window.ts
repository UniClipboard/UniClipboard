import * as HostService from '@host/hostservice'
// Window controls of the desktop host.
import { Window } from '@wailsio/runtime'
import { listen, type UnlistenFn } from './event'
import { subscribeWindowMaterial, type WindowMaterial } from './window-material'

let appliedMaterial: WindowMaterial = 'none'
subscribeWindowMaterial(material => {
  appliedMaterial = material
})

const RESIZED_EVENT = 'common:WindowDidResize'

class HostWindow {
  setDecorations = (decorations: boolean): Promise<void> =>
    HostService.SetWindowDecorations(decorations)
  // Wails has no runtime per-window theme switch; the page background is painted instead.
  setTheme = async (_theme: 'light' | 'dark' | null): Promise<void> => undefined
  // Behind a native window material the window must stay clear: an opaque window colour would hide
  // the material that the sidebar shows.
  setBackgroundColor = async (color: [number, number, number, number]): Promise<void> => {
    if (appliedMaterial !== 'none') return
    await Window.SetBackgroundColour(color[0], color[1], color[2], color[3])
  }
  isMaximized = (): Promise<boolean> => Window.IsMaximised()
  minimize = (): Promise<void> => Window.Minimise()
  maximize = (): Promise<void> => Window.Maximise()
  unmaximize = (): Promise<void> => Window.UnMaximise()
  unminimize = (): Promise<void> => Window.UnMinimise()
  close = (): Promise<void> => Window.Close()
  show = (): Promise<void> => Window.Show()
  setFocus = (): Promise<void> => Window.Focus()
  // Dragging is declared in CSS (`--wails-draggable`) by the host stylesheet.
  startDragging = async (): Promise<void> => undefined
  onResized = (handler: () => void): Promise<UnlistenFn> => listen(RESIZED_EVENT, handler)
}

const current = new HostWindow()
export const getCurrentWindow = (): HostWindow => current
