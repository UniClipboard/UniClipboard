'use client'

import { Select as SelectPrimitive } from '@base-ui/react/select'
import { ChevronDownIcon, ChevronUpIcon } from 'lucide-react'
import * as React from 'react'
import { SelectContent } from '@/components/motion/select/SelectContent'
import { SelectItem } from '@/components/motion/select/SelectItem'
import { SelectTrigger } from '@/components/motion/select/SelectTrigger'
import { cn } from '@/lib/utils'

type SelectProps<Value> = Omit<SelectPrimitive.Root.Props<Value>, 'onValueChange'> & {
  onValueChange?: (value: Value) => void
}

function Select<Value>({ onValueChange, ...props }: SelectProps<Value>) {
  const items = React.useMemo(() => collectSelectItems<Value>(props.children), [props.children])

  return (
    <SelectPrimitive.Root
      {...props}
      items={props.items ?? items}
      onValueChange={value => {
        if (value !== null) onValueChange?.(value)
      }}
    />
  )
}

function collectSelectItems<Value>(children: React.ReactNode) {
  const items: Array<{ value: Value; label: React.ReactNode }> = []

  React.Children.forEach(children, child => {
    if (!React.isValidElement<{ value?: Value; children?: React.ReactNode }>(child)) return

    if (child.type === SelectItem && child.props.value !== undefined) {
      items.push({ value: child.props.value, label: child.props.children })
      return
    }

    if (child.props.children !== undefined) {
      items.push(...collectSelectItems<Value>(child.props.children))
    }
  })

  return items
}

function SelectGroup({ className, ...props }: SelectPrimitive.Group.Props) {
  return (
    <SelectPrimitive.Group
      data-slot="select-group"
      className={cn('scroll-my-1 p-1', className)}
      {...props}
    />
  )
}

function SelectValue({ className, ...props }: SelectPrimitive.Value.Props) {
  return (
    <SelectPrimitive.Value
      data-slot="select-value"
      className={cn('flex min-w-0 flex-1 items-center gap-2 truncate text-left', className)}
      {...props}
    />
  )
}

function SelectLabel({ className, ...props }: SelectPrimitive.GroupLabel.Props) {
  return (
    <SelectPrimitive.GroupLabel
      data-slot="select-label"
      className={cn('px-1.5 py-1 text-xs text-muted-foreground', className)}
      {...props}
    />
  )
}

function SelectSeparator({ className, ...props }: SelectPrimitive.Separator.Props) {
  return (
    <SelectPrimitive.Separator
      data-slot="select-separator"
      className={cn('pointer-events-none -mx-1 my-1 h-px bg-border', className)}
      {...props}
    />
  )
}

function SelectScrollUpButton({
  className,
  ...props
}: React.ComponentProps<typeof SelectPrimitive.ScrollUpArrow>) {
  return (
    <SelectPrimitive.ScrollUpArrow
      data-slot="select-scroll-up-button"
      className={cn(
        "top-0 z-10 flex w-full cursor-default items-center justify-center bg-popover py-1 [&_svg:not([class*='size-'])]:size-4",
        className
      )}
      {...props}
    >
      <ChevronUpIcon />
    </SelectPrimitive.ScrollUpArrow>
  )
}

function SelectScrollDownButton({
  className,
  ...props
}: React.ComponentProps<typeof SelectPrimitive.ScrollDownArrow>) {
  return (
    <SelectPrimitive.ScrollDownArrow
      data-slot="select-scroll-down-button"
      className={cn(
        "bottom-0 z-10 flex w-full cursor-default items-center justify-center bg-popover py-1 [&_svg:not([class*='size-'])]:size-4",
        className
      )}
      {...props}
    >
      <ChevronDownIcon />
    </SelectPrimitive.ScrollDownArrow>
  )
}

export {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectScrollDownButton,
  SelectScrollUpButton,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
}
