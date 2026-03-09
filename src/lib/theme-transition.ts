/**
 * View Transition utility using the View Transition API.
 * Creates circular reveal / collapse animations from a given origin point.
 */

import { flushSync } from 'react-dom'

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
export function startCircularReveal(x: number | null, y: number | null, updateDOM: () => void) {
  if (x === null || y === null || !document.startViewTransition) {
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
 * Execute a DOM update wrapped in a View Transition with circular collapse.
 * The OLD view shrinks from full viewport down to (x, y).
 * This is the reverse of startCircularReveal.
 */
export function startCircularCollapse(x: number, y: number, updateDOM: () => void) {
  if (!document.startViewTransition) {
    updateDOM()
    return
  }

  const startRadius = Math.hypot(
    Math.max(x, window.innerWidth - x),
    Math.max(y, window.innerHeight - y)
  )

  // Swap z-index so old view (settings) is on top and collapses away
  document.documentElement.classList.add('view-transition-collapse')

  const transition = document.startViewTransition(() => {
    flushSync(updateDOM)
  })

  transition.ready.then(() => {
    document.documentElement.animate(
      {
        clipPath: [`circle(${startRadius}px at ${x}px ${y}px)`, `circle(0px at ${x}px ${y}px)`],
      },
      {
        duration: 500,
        easing: 'ease-in-out',
        pseudoElement: '::view-transition-old(root)',
      }
    )
  })

  transition.finished.then(() => {
    document.documentElement.classList.remove('view-transition-collapse')
  })
}

/** Get the center position of the settings icon in the sidebar */
export function getSettingsIconPosition(): { x: number; y: number } {
  const el = document.querySelector('[data-settings-icon]')
  if (el) {
    const rect = el.getBoundingClientRect()
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
  }
  // Fallback: bottom-left area where settings icon typically is
  return { x: 28, y: window.innerHeight - 28 }
}

/**
 * Execute a DOM update wrapped in a View Transition with circular reveal,
 * using the last stored click position (for theme switching).
 */
export function startThemeTransition(updateDOM: () => void) {
  startCircularReveal(lastClickX, lastClickY, updateDOM)
}
