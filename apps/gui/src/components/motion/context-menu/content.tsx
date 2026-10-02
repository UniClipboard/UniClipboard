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
import { m } from 'framer-motion'
import {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import {
  MenuPoint,
  VIEWPORT_PADDING,
  useContextMenuContext,
  getEnabledItems,
  clamp,
  collapsedClip,
  ContextMenuContentProps,
} from '@/components/motion/context-menu/state'
import {
  MENU_SURFACE_CLASS,
  MENU_SHADOW_CLASS,
  menuMotion,
} from '@/components/motion/menu/presentation'
import { cn } from '@/lib/utils'

export function ContextMenuContent({
  children,
  className,
  ariaLabel = 'Context menu',
  side = 'bottom',
}: ContextMenuContentProps) {
  const context = useContextMenuContext('ContextMenuContent')
  const [mounted, setMounted] = useState(false)
  const [position, setPosition] = useState<MenuPoint>(context.point)
  const [origin, setOrigin] = useState<MenuPoint>({ x: 0, y: 0 })
  const [size, setSize] = useState({ width: 0, height: 0 })
  const [morphReady, setMorphReady] = useState(false)
  const typeahead = useRef('')
  const typeaheadTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => setMounted(true), [])

  useLayoutEffect(() => {
    if (!context.open) {
      setMorphReady(false)
      return
    }
    const content = context.contentRef.current
    if (!content) return
    content.dataset.invocation = String(context.invocation)

    const rect = content.getBoundingClientRect()
    const triggerRect = context.parent ? context.triggerRef.current?.getBoundingClientRect() : null
    const targetX = triggerRect
      ? triggerRect.right + rect.width > window.innerWidth - VIEWPORT_PADDING
        ? triggerRect.left - rect.width + 4
        : triggerRect.right - 4
      : context.point.x
    const targetY = triggerRect
      ? triggerRect.top - 6
      : side === 'top'
        ? context.point.y - rect.height - 6
        : context.point.y
    const left = Math.max(
      VIEWPORT_PADDING,
      Math.min(
        Math.max(targetX, VIEWPORT_PADDING),
        window.innerWidth - rect.width - VIEWPORT_PADDING
      )
    )
    const top = Math.max(
      VIEWPORT_PADDING,
      Math.min(
        Math.max(targetY, VIEWPORT_PADDING),
        window.innerHeight - rect.height - VIEWPORT_PADDING
      )
    )

    setPosition({ x: left, y: top })
    setSize({ width: rect.width, height: rect.height })
    setOrigin({
      x: clamp(context.point.x - left, 12, Math.max(12, rect.width - 12)),
      y: clamp(context.point.y - top, 12, Math.max(12, rect.height - 12)),
    })
    setMorphReady(false)

    if (context.reduce || context.modality === 'keyboard') {
      setMorphReady(true)
      return
    }

    // Let the measured collapsed clip paint once before expanding it. Without
    // this preparation frame, the first invocation can batch both states and
    // appear at full size without the morph.
    let openFrame = 0
    const prepareFrame = requestAnimationFrame(() => {
      openFrame = requestAnimationFrame(() => setMorphReady(true))
    })
    return () => {
      cancelAnimationFrame(prepareFrame)
      cancelAnimationFrame(openFrame)
    }
  }, [
    side,
    mounted,
    context.open,
    context.point,
    context.contentRef,
    context.invocation,
    context.modality,
    context.reduce,
  ])

  useEffect(() => {
    if (!context.open) return
    const frame = requestAnimationFrame(() => {
      const first = getEnabledItems(context.contentRef.current)[0]
      first?.focus({ preventScroll: true })
    })
    return () => cancelAnimationFrame(frame)
  }, [mounted, context.open, context.contentRef])

  useEffect(
    () => () => {
      if (typeaheadTimer.current) clearTimeout(typeaheadTimer.current)
    },
    []
  )

  const moveFocus = (direction: 1 | -1) => {
    const items = getEnabledItems(context.contentRef.current)
    if (items.length === 0) return
    const current = items.indexOf(document.activeElement as HTMLElement)
    const next = current < 0 ? 0 : (current + direction + items.length) % items.length
    items[next]?.focus()
  }

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    // Menu navigation must not also drive the history/quick-panel shortcuts.
    event.stopPropagation()
    if (event.key === 'ArrowLeft' && context.parent) {
      event.preventDefault()
      context.setOpen(false)
      context.triggerRef.current?.focus()
      return
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      context.setOpen(false)
      context.triggerRef.current?.focus()
      return
    }
    if (event.key === 'Tab') {
      context.closeAll()
      return
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      moveFocus(event.key === 'ArrowDown' ? 1 : -1)
      return
    }
    if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault()
      const items = getEnabledItems(context.contentRef.current)
      items[event.key === 'Home' ? 0 : items.length - 1]?.focus()
      return
    }
    if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      typeahead.current += event.key.toLocaleLowerCase()
      if (typeaheadTimer.current) clearTimeout(typeaheadTimer.current)
      typeaheadTimer.current = setTimeout(() => {
        typeahead.current = ''
      }, 500)
      const match = getEnabledItems(context.contentRef.current).find(item =>
        (item.dataset.label ?? item.textContent ?? '')
          .trim()
          .toLocaleLowerCase()
          .startsWith(typeahead.current)
      )
      match?.focus()
    }
  }

  if (!mounted || (!context.open && context.invocation === 0)) return null

  const visualOpen = context.open && morphReady
  const clipHidden = collapsedClip(origin, size)

  return createPortal(
    <div
      data-context-menu-portal=""
      data-context-menu-tree={context.treeId}
      aria-hidden={!context.open}
      inert={!context.open}
      style={{ left: position.x, top: position.y }}
      className={cn(
        'fixed z-[100]',
        MENU_SHADOW_CLASS,
        context.open ? 'pointer-events-auto' : 'pointer-events-none'
      )}
    >
      <m.div
        ref={context.contentRef}
        id={context.menuId}
        role="menu"
        data-slot="context-menu-content"
        data-state={context.open ? 'open' : 'closed'}
        aria-label={ariaLabel}
        data-morph-ready={morphReady ? 'true' : 'false'}
        tabIndex={-1}
        initial={false}
        {...menuMotion(visualOpen, context.reduce, clipHidden, context.modality === 'keyboard')}
        onKeyDown={onKeyDown}
        onContextMenu={event => event.preventDefault()}
        className={cn(
          'min-w-56 max-w-[calc(100vw-1rem)] max-h-[calc(100vh-1rem)] overflow-x-hidden overflow-y-auto',
          MENU_SURFACE_CLASS,
          className
        )}
      >
        {children}
      </m.div>
    </div>,
    document.body
  )
}
