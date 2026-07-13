/**
 * View Transition utility using the View Transition API.
 * Creates a circle-blur reveal animation from a given origin point (used for
 * theme switching): an expanding circular clip-path paired with a deblur.
 */

import { flushSync } from 'react-dom'
import { isLowEffectsEnabled } from '@/lib/platform'

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
 * Pass null for x or y to skip the reveal animation (e.g. keyboard/ESC activations).
 */
function startCircularReveal(x: number | null, y: number | null, updateDOM: () => void) {
  if (x === null || y === null || isLowEffectsEnabled() || !document.startViewTransition) {
    updateDOM()
    return
  }

  const endRadius = Math.hypot(
    Math.max(x, window.innerWidth - x),
    Math.max(y, window.innerHeight - y)
  )

  const transition = document.startViewTransition(() => {
    flushSync(updateDOM)
  })

  transition.ready.then(() => {
    // "circle-blur" reveal (ported from beui.dev/components/motion/theme-toggle):
    // the new snapshot clips in as an expanding circle from the click point while
    // deblurring 8px -> 0px. globals.css already pins the old snapshot underneath
    // (animation: none, z-index) so only this reveal is visible.
    document.documentElement.animate(
      {
        clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${endRadius}px at ${x}px ${y}px)`],
        filter: ['blur(8px)', 'blur(0px)'],
      },
      {
        duration: 700,
        easing: 'cubic-bezier(0.4, 0, 0.2, 1)',
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
