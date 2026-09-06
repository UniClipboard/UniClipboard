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
import { ChevronRight } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { ContextMenuItemBase } from '@/components/motion/context-menu/item-base'
import {
  OpenModality,
  ContextMenuContext,
  useContextMenuContext,
  ContextMenuItemProps,
} from '@/components/motion/context-menu/state'

export function ContextMenuSubTrigger({
  children,
  disabled,
  textValue,
  className,
}: ContextMenuItemProps) {
  const context = useContextMenuContext('ContextMenuSubTrigger')
  const parent = context.parent
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current)
    },
    []
  )
  if (!parent) throw new Error('ContextMenuSubTrigger requires a parent menu')
  const show = (modality: OpenModality) => {
    if (timer.current) clearTimeout(timer.current)
    timer.current = null
    const trigger = context.triggerRef.current
    if (disabled || !trigger) return
    trigger.focus()
    const rect = trigger.getBoundingClientRect()
    context.openAt({ x: rect.right, y: rect.top }, modality)
  }
  return (
    <ContextMenuContext.Provider value={parent}>
      <ContextMenuItemBase
        id={`${context.menuId}-trigger`}
        ref={node => {
          context.triggerRef.current = node
        }}
        disabled={disabled}
        textValue={textValue}
        className={className}
        closeOnSelect={false}
        ariaExpanded={context.open}
        ariaControls={context.open ? context.menuId : undefined}
        onSelect={() => show('keyboard')}
        onKeyDown={event => {
          if (event.key !== 'ArrowRight') return
          event.preventDefault()
          event.stopPropagation()
          show('keyboard')
        }}
        onPointerEnter={event => {
          if (disabled || event.pointerType === 'touch' || context.open) return
          timer.current = setTimeout(() => {
            show('pointer')
            timer.current = null
          }, 120)
        }}
        onPointerLeave={() => {
          if (timer.current) clearTimeout(timer.current)
          timer.current = null
        }}
      >
        {children}
        <ChevronRight aria-hidden="true" className="ml-auto size-4" />
      </ContextMenuItemBase>
    </ContextMenuContext.Provider>
  )
}
