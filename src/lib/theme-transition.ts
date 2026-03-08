/**
 * Theme transition utility using View Transition API.
 * Creates a circular reveal animation from the click position.
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
 * Falls back to immediate execution if View Transition API is not supported.
 */
export function startThemeTransition(updateDOM: () => void) {
  if (!document.startViewTransition) {
    updateDOM()
    return
  }

  const x = lastClickX
  const y = lastClickY

  // Calculate the maximum radius needed to cover the entire viewport
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
