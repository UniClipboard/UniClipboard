/**
 * Keep the native webview context menu out of the production UI.
 *
 * Tauri dev builds retain the native menu so the platform's developer-tools
 * entry remains available for local debugging.
 */
export function initializeWebviewContextMenu(): void {
  if (import.meta.env.DEV || typeof window === 'undefined') return

  window.addEventListener('contextmenu', preventContextMenu)
}

function preventContextMenu(event: MouseEvent): void {
  event.preventDefault()
}
