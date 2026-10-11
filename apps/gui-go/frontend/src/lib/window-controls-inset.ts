import type { CSSProperties } from 'react'

/**
 * Space the app-drawn window controls (three 2.875rem buttons, 3rem tall: the
 * Win11 caption buttons of board C2) take
 * from the main window's top-right corner. The main layout publishes it as CSS
 * variables only while the controls are shown; elsewhere both resolve to 0, so
 * pages can use them unconditionally to keep their top-right content clear.
 */
export const WINDOW_CONTROLS_INSET_STYLE = {
  '--window-controls-inset-x': '8.625rem',
  '--window-controls-inset-y': '3rem',
} as CSSProperties
