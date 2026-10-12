// Window material that the native host actually put behind this page.
//
// The value comes from the host, never from a frontend guess: the Go side decides it from the macOS
// version, whether the webview can be transparent (Wails needs `-tags private_mac_apis` for that) and
// the window options it applied, and hands it to the page as the `window-material` launch parameter
// of the window URL. A browser, or a host that applied no material, reports `none`.
//
// Only the macOS skin of `@/ui-kit` and the host window module read this; business code never does.
export type WindowMaterial = 'liquid-glass' | 'visual-effect' | 'none'

// Read once at startup (src/host/main.ts imports this module first): routing drops the query string.
const reported =
  typeof window === 'undefined'
    ? null
    : new URLSearchParams(window.location.search).get('window-material')
const material: WindowMaterial =
  reported === 'liquid-glass' || reported === 'visual-effect' ? reported : 'none'

/** Calls `onChange` with the applied material; the material is fixed for the window's lifetime. */
export function subscribeWindowMaterial(onChange: (material: WindowMaterial) => void): () => void {
  onChange(material)
  return () => {}
}
