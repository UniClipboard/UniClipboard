export const HISTORY_ENTRY_ANIMATION = {
  animate: { opacity: 1, y: 0 },
  initial: { opacity: 0, y: 16 },
  transition: { type: 'spring', stiffness: 400, damping: 30 },
} as const

export const HISTORY_PREVIEW_ENTRY_TRANSITION = {
  ...HISTORY_ENTRY_ANIMATION.transition,
  delay: 0.08,
} as const
