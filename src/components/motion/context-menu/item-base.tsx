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
import { type Ref, useId } from 'react'
import { useContextMenuContext, ContextMenuItemProps } from '@/components/motion/context-menu/state'
import { SPRING_LAYOUT } from '@/lib/ease'
import { cn } from '@/lib/utils'

export function ContextMenuItemBase({
  children,
  onSelect,
  disabled = false,
  closeOnSelect = true,
  tone = 'default',
  inset = false,
  className,
  textValue,
  role = 'menuitem',
  ariaChecked,
  id: providedId,
  ref,
  onKeyDown,
  onPointerEnter,
  onPointerLeave,
  ariaExpanded,
  ariaControls,
}: ContextMenuItemProps & {
  role?: 'menuitem' | 'menuitemcheckbox' | 'menuitemradio'
  ariaChecked?: boolean
  id?: string
  ref?: Ref<HTMLButtonElement>
  onKeyDown?: React.KeyboardEventHandler<HTMLButtonElement>
  onPointerEnter?: React.PointerEventHandler<HTMLButtonElement>
  onPointerLeave?: React.PointerEventHandler<HTMLButtonElement>
  ariaExpanded?: boolean
  ariaControls?: string
}) {
  const context = useContextMenuContext('ContextMenuItem')
  const generatedId = useId()
  const id = providedId ?? generatedId
  const active = context.activeId === id
  const checkedProps = role === 'menuitem' ? {} : { 'aria-checked': ariaChecked }

  return (
    <button
      type="button"
      id={id}
      ref={ref}
      role={role}
      aria-haspopup={ariaExpanded !== undefined ? 'menu' : undefined}
      aria-expanded={ariaExpanded}
      aria-controls={ariaControls}
      onKeyDown={onKeyDown}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
      {...checkedProps}
      disabled={disabled}
      data-context-menu-item="true"
      data-disabled={disabled ? 'true' : undefined}
      data-label={textValue}
      tabIndex={-1}
      onFocus={() => context.setActiveId(id)}
      onPointerMove={event => {
        if (!disabled && event.pointerType !== 'touch') event.currentTarget.focus()
      }}
      onClick={() => {
        if (disabled) return
        onSelect?.()
        if (closeOnSelect) context.closeAll()
      }}
      className={cn(
        'relative isolate flex w-full select-none items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[0.8125rem] outline-none',
        'focus-visible:ring-2 focus-visible:ring-foreground/15',
        'disabled:pointer-events-none disabled:opacity-40',
        inset && 'pl-8',
        tone === 'destructive' ? 'text-destructive' : 'text-foreground',
        className
      )}
    >
      {active ? (
        <m.span
          layoutId={`${context.menuId}-active`}
          className={cn(
            'absolute inset-0 -z-10 rounded-lg',
            tone === 'destructive' ? 'bg-destructive/10' : 'bg-foreground/[0.065]'
          )}
          transition={context.reduce ? { duration: 0 } : SPRING_LAYOUT}
        />
      ) : null}
      {children}
    </button>
  )
}
