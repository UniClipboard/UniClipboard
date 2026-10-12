// The design (Main.dc.html) seats the traffic lights at the top of the
// full-height Library sidebar rather than in a 40pt title bar.
export const MAC_SIDEBAR_TRAFFIC_LIGHT_OFFSET = { x: 4, y: 8 } as const
// With the sidebar collapsed the lights sit in the top band over the icon
// column, centered on the first content column's 44px header controls.
export const MAC_CONTENT_TRAFFIC_LIGHT_OFFSET = { x: 4, y: 16 } as const
