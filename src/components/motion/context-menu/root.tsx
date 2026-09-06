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
import { useReducedMotion } from 'framer-motion'
import { useCallback, use, useEffect, useId, useMemo, useRef, useState } from 'react'
import {
  OpenModality,
  MenuPoint,
  ContextMenuContextValue,
  ContextMenuContext,
  ContextMenuProps,
} from '@/components/motion/context-menu/state'
import { cn } from '@/lib/utils'

export function ContextMenu({
  children,
  open: controlledOpen,
  defaultOpen = false,
  onOpenChange,
  className,
}: ContextMenuProps) {
  const parent = use(ContextMenuContext)
  const [internalOpen, setInternalOpen] = useState(defaultOpen)
  const [point, setPoint] = useState<MenuPoint>({ x: 0, y: 0 })
  const [modality, setModality] = useState<OpenModality>('pointer')
  const [invocation, setInvocation] = useState(0)
  const [{ activeId, submenuId }, setSelection] = useState<{
    activeId: string | null
    submenuId: string | null
  }>({ activeId: null, submenuId: null })
  const setActiveId = useCallback((id: string | null) => {
    setSelection(current => (current.activeId === id ? current : { activeId: id, submenuId: null }))
  }, [])
  const setSubmenuId = useCallback((id: string | null) => {
    setSelection(current => ({ activeId: id ?? current.activeId, submenuId: id }))
  }, [])
  const controlled = controlledOpen !== undefined
  const triggerRef = useRef<HTMLElement | null>(null)
  const contentRef = useRef<HTMLDivElement | null>(null)
  const menuId = useId()
  const treeId = parent?.treeId ?? menuId
  // The parent owns its open submenu. Moving to another row clears that
  // selection, so returning cannot revive an old open request.
  const open = parent
    ? parent.open && parent.submenuId === `${menuId}-trigger`
    : controlled
      ? controlledOpen
      : internalOpen
  const parentSetSubmenuId = parent?.setSubmenuId
  const reduce = useReducedMotion() ?? false

  const setOpen = useCallback(
    (next: boolean) => {
      if (parentSetSubmenuId) parentSetSubmenuId(next ? `${menuId}-trigger` : null)
      else if (!controlled) setInternalOpen(next)
      onOpenChange?.(next)
      if (!next) setActiveId(null)
    },
    [controlled, onOpenChange, parentSetSubmenuId, menuId, setActiveId]
  )

  const parentCloseAll = parent?.closeAll
  const closeAll = useCallback(() => {
    setOpen(false)
    parentCloseAll?.()
  }, [setOpen, parentCloseAll])

  const openAt = useCallback(
    (nextPoint: MenuPoint, nextModality: OpenModality) => {
      setPoint(nextPoint)
      setModality(nextModality)
      setInvocation(current => current + 1)
      setActiveId(null)
      setOpen(true)
    },
    [setOpen, setActiveId]
  )

  useEffect(() => {
    if (!open) return

    const onPointerDown = (event: PointerEvent) => {
      const target = event.target
      const portal = target instanceof Element ? target.closest('[data-context-menu-tree]') : null
      if (portal?.getAttribute('data-context-menu-tree') !== treeId) setOpen(false)
    }
    const onWindowChange = () => setOpen(false)

    window.addEventListener('pointerdown', onPointerDown)
    window.addEventListener('resize', onWindowChange)
    window.addEventListener('scroll', onWindowChange)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown)
      window.removeEventListener('resize', onWindowChange)
      window.removeEventListener('scroll', onWindowChange)
    }
  }, [open, setOpen, treeId])

  const value = useMemo<ContextMenuContextValue>(
    () => ({
      parent,
      treeId,
      closeAll,
      open,
      setOpen,
      openAt,
      point,
      modality,
      invocation,
      menuId,
      triggerRef,
      contentRef,
      activeId,
      setActiveId,
      submenuId,
      setSubmenuId,
      reduce,
    }),
    [
      parent,
      treeId,
      closeAll,
      open,
      setOpen,
      openAt,
      point,
      modality,
      invocation,
      menuId,
      activeId,
      setActiveId,
      submenuId,
      setSubmenuId,
      reduce,
    ]
  )

  return (
    <ContextMenuContext.Provider value={value}>
      <div className={cn('contents', className)}>{children}</div>
    </ContextMenuContext.Provider>
  )
}
