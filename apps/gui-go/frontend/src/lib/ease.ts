// Shared motion parameters for active transitions.
export const EASE_OUT = [0.16, 1, 0.3, 1] as const

/** Overlay panel entrances. */
export const SPRING_PANEL = {
  type: 'spring',
  stiffness: 420,
  damping: 40,
  mass: 0.5,
} as const

/** Shared-layout transitions. */
export const SPRING_LAYOUT = {
  type: 'spring',
  stiffness: 360,
  damping: 32,
  mass: 0.6,
} as const
