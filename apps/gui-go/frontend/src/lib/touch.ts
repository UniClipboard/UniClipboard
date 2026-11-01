/** Preserve mouse selection; suppress selection for the primary coarse pointer. */
export const TOUCH_GESTURE_CONTENT_CLASS = '[-webkit-touch-callout:none] pointer-coarse:select-none'

/** Suppress selection during a gesture, including on hybrid input devices. */
export function holdSelection(element: HTMLElement) {
  element.style.setProperty('user-select', 'none')
  element.style.setProperty('-webkit-user-select', 'none')
  return () => {
    element.style.removeProperty('user-select')
    element.style.removeProperty('-webkit-user-select')
  }
}

/** Detect hover from the event, including pen contact via `buttons`. */
export const isHoveringPointer = (event: { pointerType: string; buttons: number }) =>
  event.pointerType !== 'touch' && event.buttons === 0
