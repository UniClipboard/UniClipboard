import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { usePanelRef } from 'react-resizable-panels'
import {
  LIST_COLUMN,
  readListWidth,
  useHistoryLayoutTier,
  writeListWidth,
  type HistoryLayoutTier,
} from './history-layout'

/**
 * Sizing of the macOS list column across window tiers: per-tier constraints,
 * the user's remembered width per tier, and a double-click reset (via
 * `defaultSize`) to the tier default.
 *
 * The column tracks an intended width — the remembered one, else the tier
 * default — and re-applies it whenever the panel group changes size. The
 * library then clamps it against the detail column's floor, so a window that
 * shrinks and grows back returns to the intended width instead of keeping the
 * clamped one. Only a width the user dragged to is remembered.
 */
export function useHistoryListColumn() {
  const tier = useHistoryLayoutTier()
  const panelRef = usePanelRef()
  const [groupElement, setGroupElement] = useState<HTMLDivElement | null>(null)
  const tierRef = useRef<HistoryLayoutTier>(tier)
  const userResizing = useRef(false)

  const applyIntendedWidth = useCallback(() => {
    const panel = panelRef.current
    // An unmeasured group (hidden window) cannot be resized; defaultSize holds.
    if (!panel || panel.getSize().inPixels === 0) return
    const intended = readListWidth(tierRef.current) ?? LIST_COLUMN[tierRef.current].default
    if (Math.abs(panel.getSize().inPixels - intended) > 1) panel.resize(`${intended}px`)
  }, [panelRef])

  // Layout effect: the panel registers its new constraints in its own layout
  // effect first, so this lands before paint, with no default-width flash.
  useLayoutEffect(() => {
    tierRef.current = tier
    applyIntendedWidth()
  }, [applyIntendedWidth, tier])

  // The group's own resize also moves with the sidebar switching between rail
  // and full width. Re-apply a frame later, once the library has measured the
  // new group size; resizing a panel does not resize the group, so no loop.
  useEffect(() => {
    if (!groupElement) return
    let frame = 0
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(applyIntendedWidth)
    })
    observer.observe(groupElement)
    return () => {
      cancelAnimationFrame(frame)
      observer.disconnect()
    }
  }, [applyIntendedWidth, groupElement])

  const markUserResize = useCallback(() => {
    userResizing.current = true
    // `onLayoutChanged` fires on release; drop the mark after it so a later
    // window resize is not taken for a drag.
    window.addEventListener(
      'pointerup',
      () => requestAnimationFrame(() => (userResizing.current = false)),
      { once: true }
    )
  }, [])

  const markKeyboardResize = useCallback(() => {
    userResizing.current = true
    requestAnimationFrame(() => (userResizing.current = false))
  }, [])

  const onLayoutChanged = useCallback(() => {
    if (!userResizing.current) return
    const size = panelRef.current?.getSize()
    if (size) writeListWidth(tierRef.current, size.inPixels)
  }, [panelRef])

  const column = LIST_COLUMN[tier]
  return {
    tier,
    groupProps: { elementRef: setGroupElement, onLayoutChanged },
    panelProps: {
      panelRef,
      defaultSize: `${column.default}px`,
      minSize: `${column.min}px`,
      maxSize: `${column.max}px`,
      groupResizeBehavior: 'preserve-pixel-size' as const,
    },
    handleProps: { onPointerDown: markUserResize, onKeyDown: markKeyboardResize },
  }
}
