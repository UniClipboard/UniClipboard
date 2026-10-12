import type { KeyboardEvent } from 'react'

/**
 * A key that belongs to an open input-method composition (Chinese, Japanese): Enter commits the
 * candidate and the arrows move through candidates, so fields must not act on it. WebKit can report
 * `isComposing === false` on the committing keydown, hence the 229 key code as well.
 */
export function isImeKey(event: KeyboardEvent) {
  return event.nativeEvent.isComposing || event.keyCode === 229
}
