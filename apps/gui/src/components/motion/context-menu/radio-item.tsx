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
import { use } from 'react'
import { ContextMenuItemBase } from '@/components/motion/context-menu/item-base'
import {
  ContextMenuRadioGroupContext,
  ContextMenuRadioItemProps,
} from '@/components/motion/context-menu/state'
import { cn } from '@/lib/utils'

export function ContextMenuRadioItem({ value, children, ...props }: ContextMenuRadioItemProps) {
  const group = use(ContextMenuRadioGroupContext)
  if (!group) {
    throw new Error('ContextMenuRadioItem must be used within <ContextMenuRadioGroup>')
  }
  const checked = group.value === value
  return (
    <ContextMenuItemBase
      {...props}
      role="menuitemradio"
      ariaChecked={checked}
      onSelect={() => group.onValueChange?.(value)}
    >
      <span className="flex size-4 shrink-0 items-center justify-center">
        <span
          className={cn(
            'size-1.5 rounded-full bg-current transition-opacity',
            checked ? 'opacity-100' : 'opacity-0'
          )}
        />
      </span>
      {children}
    </ContextMenuItemBase>
  )
}
