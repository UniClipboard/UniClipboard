/**
 * View Transition utility using the View Transition API.
 * Creates a circular reveal animation from a given origin point.
 */

let lastClickX = 0
let lastClickY = 0

/** Store the click position for the next theme transition */
export function setTransitionOrigin(x: number, y: number) {
  lastClickX = x
  lastClickY = y
}

/**
 * Execute a DOM update wrapped in a View Transition with circular reveal.
 * Animates from (x, y) outward to cover the entire viewport.
 * Falls back to immediate execution if View Transition API is not supported.
 */
export function startCircularReveal(x: number, y: number, updateDOM: () => void) {
  if (!document.startViewTransition) {
    updateDOM()
    return
  }

  const endRadius = Math.hypot(
    Math.max(x, window.innerWidth - x),
    Math.max(y, window.innerHeight - y)
  )

  const transition = document.startViewTransition(updateDOM)

  transition.ready.then(() => {
    document.documentElement.animate(
      {
        clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${endRadius}px at ${x}px ${y}px)`],
      },
      {
        duration: 500,
        easing: 'ease-in-out',
        pseudoElement: '::view-transition-new(root)',
      }
    )
  })
}

/**
 * Execute a DOM update wrapped in a View Transition with circular reveal,
 * using the last stored click position (for theme switching).
 */
export function startThemeTransition(updateDOM: () => void) {
  startCircularReveal(lastClickX, lastClickY, updateDOM)
}
