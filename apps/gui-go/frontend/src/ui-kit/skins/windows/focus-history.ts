// The elements that most recently had focus, newest last. A dialog opened from a menu item finds
// its opener unmounted (the menu closed in the same commit); focus then returns to the newest
// element that is still in the document, usually the menu's trigger or the right-clicked row.
const history: HTMLElement[] = []
const limit = 8
let tracking = 0

const record = (event: FocusEvent) => {
  if (!(event.target instanceof HTMLElement)) return
  const index = history.indexOf(event.target)
  if (index >= 0) history.splice(index, 1)
  history.push(event.target)
  if (history.length > limit) history.shift()
}

/** Starts recording while at least one caller needs it; returns the matching stop function. */
export function trackFocusHistory(): () => void {
  if (tracking++ === 0) document.addEventListener('focusin', record, true)
  return () => {
    if (--tracking === 0) {
      document.removeEventListener('focusin', record, true)
      history.length = 0
    }
  }
}

/** The newest recorded element that is still connected and outside `exclude`. */
export function lastConnectedFocus(exclude?: Element | null): HTMLElement | null {
  for (let i = history.length - 1; i >= 0; i--) {
    const element = history[i]
    if (element.isConnected && element !== document.body && !exclude?.contains(element))
      return element
  }
  return null
}
