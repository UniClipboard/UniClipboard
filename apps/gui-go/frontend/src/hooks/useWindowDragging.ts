import { useRef } from 'react'
import { getCurrentWindow } from '@/host/window'

/**
 * Pointer handlers that start a native window drag once the pointer moves a
 * few pixels from a primary-button press. Complements `data-tauri-drag-region`,
 * which only fires for presses that land on the region element itself; capture
 * handlers also cover presses on its non-interactive children.
 */
export function useWindowDragging() {
  const dragStartRef = useRef<{ x: number; y: number } | null>(null)

  return {
    onPointerDownCapture: (event: React.PointerEvent<HTMLElement>) => {
      if (event.button !== 0) return
      dragStartRef.current = { x: event.clientX, y: event.clientY }
    },
    onPointerMoveCapture: (event: React.PointerEvent<HTMLElement>) => {
      const start = dragStartRef.current
      if (!start || (event.buttons & 1) === 0) return
      if (Math.hypot(event.clientX - start.x, event.clientY - start.y) < 4) return
      dragStartRef.current = null
      void getCurrentWindow()
        .startDragging()
        .catch(() => undefined)
    },
    onPointerUpCapture: () => {
      dragStartRef.current = null
    },
  }
}
