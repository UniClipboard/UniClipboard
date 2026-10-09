import type { CSSProperties } from 'react'

/**
 * Space the app-drawn window controls (three 3rem buttons, 2.5rem tall) take
 * from the main window's top-right corner. The main layout publishes it as CSS
 * variables only while the controls are shown; elsewhere both resolve to 0, so
 * pages can use them unconditionally to keep their top-right content clear.
 */
export const WINDOW_CONTROLS_INSET_STYLE = {
  '--window-controls-inset-x': '9rem',
  '--window-controls-inset-y': '2.5rem',
} as CSSProperties
