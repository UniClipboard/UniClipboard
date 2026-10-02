/**
 * Scroll offsets of the settings categories for the current window session.
 *
 * The state is deliberately kept in the module heap: closing the main window
 * destroys its webview (see `uc-tauri::run` `CloseRequested`), so reopening from
 * the tray starts with an empty heap and every category opens at the top again.
 * Nothing is persisted, and leaving the settings page inside the same window
 * keeps the offsets.
 */
const offsets = new Map<string, number>()

export function readSettingsScrollOffset(category: string): number {
  return offsets.get(category) ?? 0
}

export function rememberSettingsScrollOffset(category: string, offset: number): void {
  offsets.set(category, Math.max(0, offset))
}

export function clearSettingsScrollOffsets(): void {
  offsets.clear()
}
