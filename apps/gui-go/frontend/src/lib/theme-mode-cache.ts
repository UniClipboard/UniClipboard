export const THEME_MODE_STORAGE_KEY = 'uniclipboard.themeMode'

export type ThemeMode = 'light' | 'dark'

/**
 * The last resolved light/dark mode, kept so the first paint can match it before settings or
 * desktop-theme IPC are available. `apps/gui-go/frontend/index.html` reads the same key inline.
 */
export function readCachedThemeMode(): ThemeMode {
  try {
    const stored = window.localStorage.getItem(THEME_MODE_STORAGE_KEY)
    if (stored === 'light' || stored === 'dark') return stored
  } catch {
    /* Storage can be unavailable; fall back to the system preference. */
  }
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

export function writeCachedThemeMode(mode: ThemeMode): void {
  try {
    window.localStorage.setItem(THEME_MODE_STORAGE_KEY, mode)
  } catch {
    /* Best effort only. */
  }
}
