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
import { createContext, type ReactElement, type ReactNode, type Ref, use } from 'react'

export type OpenModality = 'pointer' | 'keyboard' | 'touch'

export type MenuPoint = { x: number; y: number }

export const VIEWPORT_PADDING = 8

export const LONG_PRESS_DELAY = 520

export const LONG_PRESS_TOLERANCE = 10

export type TriggerElementProps = React.HTMLAttributes<HTMLElement> & {
  'data-context-menu-tree'?: string
  ref?: Ref<HTMLElement>
}

export interface ContextMenuContextValue {
  parent: ContextMenuContextValue | null
  treeId: string
  closeAll: () => void
  open: boolean
  setOpen: (open: boolean) => void
  openAt: (point: MenuPoint, modality: OpenModality) => void
  point: MenuPoint
  modality: OpenModality
  invocation: number
  menuId: string
  triggerRef: React.RefObject<HTMLElement | null>
  contentRef: React.RefObject<HTMLDivElement | null>
  submenuId: string | null
  setSubmenuId: (id: string | null) => void
  activeId: string | null
  setActiveId: (id: string | null) => void
  reduce: boolean
}

export const ContextMenuContext = createContext<ContextMenuContextValue | null>(null)

export function useContextMenuContext(component: string) {
  const context = use(ContextMenuContext)
  if (!context) {
    throw new Error(`${component} must be used within <ContextMenu>`)
  }
  return context
}

export function assignRef<T>(ref: Ref<T> | undefined, value: T | null) {
  if (typeof ref === 'function') {
    ref(value)
  } else if (ref) {
    ref.current = value
  }
}

export function getEnabledItems(container: HTMLElement | null) {
  if (!container) return []
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      '[data-context-menu-item="true"]:not([data-disabled="true"])'
    )
  )
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max)
}

export function collapsedClip(origin: MenuPoint, size: { width: number; height: number }) {
  const half = 8
  const top = clamp(origin.y - half, 0, size.height)
  const right = clamp(size.width - origin.x - half, 0, size.width)
  const bottom = clamp(size.height - origin.y - half, 0, size.height)
  const left = clamp(origin.x - half, 0, size.width)
  return `inset(${top}px ${right}px ${bottom}px ${left}px round 10px)`
}

export interface ContextMenuProps {
  children: ReactNode
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
  className?: string
}

export interface ContextMenuTriggerProps {
  activation?: 'contextmenu' | 'click'
  children: ReactElement<TriggerElementProps>
  disabled?: boolean
  className?: string
}

export interface ContextMenuContentProps {
  side?: 'bottom' | 'top'
  children: ReactNode
  className?: string
  ariaLabel?: string
}

export type ContextMenuItemTone = 'default' | 'destructive'

export interface ContextMenuItemProps {
  children: ReactNode
  onSelect?: () => void
  disabled?: boolean
  closeOnSelect?: boolean
  tone?: ContextMenuItemTone
  inset?: boolean
  className?: string
  textValue?: string
}

export interface ContextMenuCheckboxItemProps extends Omit<ContextMenuItemProps, 'onSelect'> {
  checked: boolean
  onCheckedChange?: (checked: boolean) => void
}

export interface ContextMenuRadioGroupContextValue {
  value: string
  onValueChange?: (value: string) => void
}

export const ContextMenuRadioGroupContext = createContext<ContextMenuRadioGroupContextValue | null>(
  null
)

export interface ContextMenuRadioGroupProps {
  value: string
  onValueChange?: (value: string) => void
  children: ReactNode
  className?: string
}

export interface ContextMenuRadioItemProps extends Omit<ContextMenuItemProps, 'onSelect'> {
  value: string
}

export interface ContextMenuLabelProps {
  children: ReactNode
  inset?: boolean
  className?: string
}

export interface ContextMenuSeparatorProps {
  className?: string
}

export interface ContextMenuShortcutProps {
  children: ReactNode
  className?: string
}
