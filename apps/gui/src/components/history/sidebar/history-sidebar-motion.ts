/** One shared-layout id for the toggle in the expanded sidebar's top strip and
 * in the collapsed icon column: on show/hide the button glides between the two
 * spots instead of jumping. */
export const LIBRARY_TOGGLE_LAYOUT_ID = 'library-sidebar-toggle'

/** Shared by the toggle's glide and the sidebar's width change, so the two
 * land together. */
export const LIBRARY_SIDEBAR_TRANSITION = { type: 'spring', stiffness: 520, damping: 42 } as const

/** The hover peek growing out of the icon column. Eased in and out rather
 * than sprung: a spring front-loads the motion (most of the width lands in
 * the first ~150ms), which reads as the panel snapping open. */
export const SIDEBAR_PEEK_TRANSITION = { duration: 0.4, ease: [0.65, 0, 0.35, 1] } as const

/** Full sidebar (w-55) and collapsed icon column (w-13), in px for the width
 * animation. */
export const SIDEBAR_PANEL_WIDTH = 220
export const SIDEBAR_RAIL_WIDTH = 52

/** Height of the collapsed sidebar's top band (h-16): the traffic lights'
 * strip, level with the first content column's search row. */
export const SIDEBAR_BAND_HEIGHT = 64

/** Hovering the collapsed toggle waits this long before peeking, so a pointer
 * passing over it does not flash the panel. */
export const SIDEBAR_PEEK_DELAY_MS = 120
