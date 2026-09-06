'use client'

import { Popover as PopoverPrimitive } from '@base-ui/react/popover'
import { m, useReducedMotion, type HTMLMotionProps } from 'framer-motion'
import * as React from 'react'
import {
  anchoredMenuClip,
  MENU_SURFACE_CLASS,
  MENU_SHADOW_CLASS,
  menuMotion,
} from '@/components/motion/menu/presentation'
import { cn } from '@/lib/utils'

function Popover({ ...props }: PopoverPrimitive.Root.Props) {
  return <PopoverPrimitive.Root data-slot="popover" {...props} />
}

function PopoverTrigger({ ...props }: PopoverPrimitive.Trigger.Props) {
  return <PopoverPrimitive.Trigger data-slot="popover-trigger" {...props} />
}

function PopoverContent({
  className,
  align = 'center',
  alignOffset = 0,
  side = 'bottom',
  sideOffset = 8,
  finalFocus = false,
  ...props
}: PopoverPrimitive.Popup.Props &
  Pick<PopoverPrimitive.Positioner.Props, 'align' | 'alignOffset' | 'side' | 'sideOffset'>) {
  const reduce = useReducedMotion() ?? false
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Positioner
        align={align}
        alignOffset={alignOffset}
        side={side}
        sideOffset={sideOffset}
        className={cn('isolate z-50', MENU_SHADOW_CLASS)}
      >
        <PopoverPrimitive.Popup
          data-slot="popover-content"
          className={cn(
            MENU_SURFACE_CLASS,
            'z-50 flex w-72 max-w-[calc(100vw-1rem)] max-h-(--available-height) origin-(--transform-origin) flex-col gap-2.5 overflow-y-auto p-3 text-sm',

            className
          )}
          render={(renderProps, state) => {
            const collapsed = anchoredMenuClip(state.side)
            return (
              <m.div
                {...(renderProps as HTMLMotionProps<'div'>)}
                initial={menuMotion(false, reduce, collapsed).animate}
                {...menuMotion(state.open, reduce, collapsed)}
              />
            )
          }}
          finalFocus={finalFocus}
          {...props}
        />
      </PopoverPrimitive.Positioner>
    </PopoverPrimitive.Portal>
  )
}

function PopoverHeader({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      data-slot="popover-header"
      className={cn('flex flex-col gap-0.5 text-sm', className)}
      {...props}
    />
  )
}

function PopoverTitle({ className, ...props }: PopoverPrimitive.Title.Props) {
  return (
    <PopoverPrimitive.Title
      data-slot="popover-title"
      className={cn('font-medium', className)}
      {...props}
    />
  )
}

function PopoverDescription({ className, ...props }: PopoverPrimitive.Description.Props) {
  return (
    <PopoverPrimitive.Description
      data-slot="popover-description"
      className={cn('text-muted-foreground', className)}
      {...props}
    />
  )
}

export { Popover, PopoverContent, PopoverDescription, PopoverHeader, PopoverTitle, PopoverTrigger }
