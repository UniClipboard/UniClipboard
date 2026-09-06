/*!
MIT License

Copyright (c) 2026 Saurabh Chauhan

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/
// Animated Context Menu from https://beui.dev/r/context-menu/raw (MIT).
import {
  cloneElement,
  isValidElement,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  useCallback,
  useEffect,
  useRef,
} from 'react'
import {
  MenuPoint,
  LONG_PRESS_DELAY,
  LONG_PRESS_TOLERANCE,
  useContextMenuContext,
  assignRef,
  ContextMenuTriggerProps,
} from '@/components/motion/context-menu/state'
import { holdSelection, TOUCH_GESTURE_CONTENT_CLASS } from '@/lib/touch'
import { cn } from '@/lib/utils'

export function ContextMenuTrigger({
  children,
  disabled = false,
  className,
}: ContextMenuTriggerProps) {
  const context = useContextMenuContext('ContextMenuTrigger')
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const touchOrigin = useRef<MenuPoint | null>(null)
  const releaseSelection = useRef<(() => void) | null>(null)

  const cancelLongPress = useCallback(() => {
    if (longPressTimer.current) {
      clearTimeout(longPressTimer.current)
      longPressTimer.current = null
    }
    touchOrigin.current = null
  }, [])

  // Held for the whole press, not just the timer: a gesture that turned into a
  // drag must not paint a selection under the finger either.
  const endPress = useCallback(() => {
    cancelLongPress()
    releaseSelection.current?.()
    releaseSelection.current = null
  }, [cancelLongPress])

  useEffect(() => endPress, [endPress])

  if (!isValidElement(children)) {
    throw new Error('<ContextMenuTrigger> requires a single React element')
  }

  const childProps = children.props
  const childRef = children.props.ref

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    childProps.onPointerDown?.(event)
    // A pen presses the same way a finger does and gets no `contextmenu` out
    // of the platform for it, so it holds to open too. A mouse has the right
    // button and is left to `onContextMenu`.
    const pressToOpen = event.pointerType === 'touch' || event.pointerType === 'pen'
    if (event.defaultPrevented || disabled || !pressToOpen) return

    // `pointer-coarse:select-none` misses this press on a laptop whose mouse
    // is the primary pointer and whose touchscreen is not, and the platform's
    // own long-press selection then claims the gesture and cancels ours. The
    // press is the only thing that knows which input is on the glass, so it
    // takes selection away itself — for its own duration, and no longer.
    releaseSelection.current?.()
    releaseSelection.current = holdSelection(event.currentTarget)

    const origin = { x: event.clientX, y: event.clientY }
    touchOrigin.current = origin
    longPressTimer.current = setTimeout(() => {
      context.openAt(origin, 'touch')
      longPressTimer.current = null
      touchOrigin.current = null
    }, LONG_PRESS_DELAY)
  }

  const onPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    childProps.onPointerMove?.(event)
    const origin = touchOrigin.current
    if (
      origin &&
      Math.hypot(event.clientX - origin.x, event.clientY - origin.y) > LONG_PRESS_TOLERANCE
    ) {
      cancelLongPress()
    }
  }

  const onKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    childProps.onKeyDown?.(event)
    if (event.defaultPrevented || disabled) return
    if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return

    event.preventDefault()
    const rect = event.currentTarget.getBoundingClientRect()
    context.openAt(
      { x: rect.left + Math.min(24, rect.width / 2), y: rect.top + rect.height / 2 },
      'keyboard'
    )
  }

  return cloneElement(children, {
    ref: (node: HTMLElement | null) => {
      context.triggerRef.current = node
      assignRef(childRef, node)
    },
    'aria-controls': context.open ? context.menuId : undefined,
    'aria-haspopup': 'menu',
    'aria-expanded': context.open,
    // The long press is ours: without this iOS runs its own on the same
    // gesture and drops the selection callout and its handles on top of the
    // menu we just opened. Only the press gesture is ours though — the child
    // is the consumer's content, so a mouse can still select the text in it
    // and right-click the selection. `touch-none` stays off too: the page
    // still has to scroll from the trigger.
    className: cn(TOUCH_GESTURE_CONTENT_CLASS, childProps.className, className),
    onContextMenu: (event: ReactMouseEvent<HTMLElement>) => {
      childProps.onContextMenu?.(event)
      if (event.defaultPrevented || disabled) return
      event.preventDefault()
      endPress()
      context.openAt({ x: event.clientX, y: event.clientY }, 'pointer')
    },
    onKeyDown,
    onPointerDown,
    onPointerMove,
    onPointerUp: (event: ReactPointerEvent<HTMLElement>) => {
      childProps.onPointerUp?.(event)
      endPress()
    },
    onPointerCancel: (event: ReactPointerEvent<HTMLElement>) => {
      childProps.onPointerCancel?.(event)
      endPress()
    },
  })
}
