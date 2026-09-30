import { getCurrentWindow } from '@tauri-apps/api/window'
import { useMemo, useRef, type PointerEvent as ReactPointerEvent } from 'react'
import { usePlatform } from '@/hooks/usePlatform'
import { createLogger } from '@/lib/logger'

const log = createLogger('window-drag')

const DRAG_THRESHOLD_PX = 4

// Nested drag surfaces (a title bar inside a sidebar) each run this hook and
// receive the same native event; only the first may request the drag.
const handledEvents = new WeakSet<Event>()

// Mirrors the elements Tauri's injected drag script treats as interactive, plus
// explicit opt-outs, so a press on a control never starts a window drag.
const INTERACTIVE_SELECTOR = [
  'a',
  'button',
  'input',
  'select',
  'textarea',
  'label',
  'summary',
  '[role="button"]',
  '[role="link"]',
  '[role="menuitem"]',
  '[role="tab"]',
  '[role="checkbox"]',
  '[role="radio"]',
  '[role="switch"]',
  '[role="option"]',
  '[contenteditable]:not([contenteditable="false"])',
  '[data-tauri-drag-region="false"]',
].join(',')

interface WindowDragOptions {
  /** Start dragging even when the press lands on an interactive child. */
  allowInteractive?: boolean
}

/**
 * Pointer-event based window dragging for custom title bars on Linux.
 *
 * Tauri's built-in `data-tauri-drag-region` script only listens to `mousedown`.
 * Pointer devices that WebKitGTK does not report through `mousedown` (for
 * example an absolute-coordinate digitizer, as used by UTM guests) never reach
 * it, so the window cannot be moved. Pointer events cover every device class.
 * The drag starts once the pointer moves past a small threshold with the
 * primary button held, so plain clicks are unaffected.
 */
export function useWindowDrag({ allowInteractive = false }: WindowDragOptions = {}) {
  const { isLinux, isTauri } = usePlatform()
  const startRef = useRef<{ x: number; y: number } | null>(null)
  const enabled = isLinux && isTauri

  return useMemo(() => {
    if (!enabled) return {}

    const reset = () => {
      startRef.current = null
    }

    return {
      onPointerDownCapture: (event: ReactPointerEvent<HTMLElement>) => {
        if (event.button !== 0) return
        const target = event.target as Element | null
        if (!allowInteractive && target?.closest(INTERACTIVE_SELECTOR)) return
        startRef.current = { x: event.clientX, y: event.clientY }
      },
      onPointerMoveCapture: (event: ReactPointerEvent<HTMLElement>) => {
        const start = startRef.current
        if (!start || (event.buttons & 1) === 0) return
        if (Math.hypot(event.clientX - start.x, event.clientY - start.y) < DRAG_THRESHOLD_PX) return
        reset()
        if (handledEvents.has(event.nativeEvent)) return
        handledEvents.add(event.nativeEvent)
        void getCurrentWindow()
          .startDragging()
          .catch(error => log.warn({ err: error }, 'Failed to start window drag'))
      },
      onPointerUpCapture: reset,
      onPointerCancelCapture: reset,
    }
  }, [allowInteractive, enabled])
}
