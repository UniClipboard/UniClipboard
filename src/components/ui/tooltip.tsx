'use client'

import { Tooltip as TooltipPrimitive } from 'radix-ui'
import * as React from 'react'
import { cn } from '@/lib/utils'

function TooltipProvider({
  delayDuration = 0,
  disableHoverableContent = true,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Provider>) {
  return (
    <TooltipPrimitive.Provider
      data-slot="tooltip-provider"
      delayDuration={delayDuration}
      disableHoverableContent={disableHoverableContent}
      {...props}
    />
  )
}

function Tooltip({
  open: openProp,
  onOpenChange: onOpenChangeProp,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Root>) {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(false)

  const isControlled = openProp !== undefined
  const open = isControlled ? openProp : uncontrolledOpen
  const onOpenChange = isControlled ? onOpenChangeProp : setUncontrolledOpen

  React.useEffect(() => {
    const handleBlur = () => {
      // 关闭已打开的 tooltip，避免切回窗口后旧的 tooltip 仍残留。
      if (open) onOpenChange?.(false)
      // 触发器在被点击后会保留 DOM 焦点。Windows 在窗口重新获焦时会向
      // 上一次获焦的元素再分发一次 focus 事件，Radix Tooltip 的 onFocus
      // 处理器随即 onOpen，导致 tooltip 在用户没有 hover 时自动展开。
      // 这里在窗口失焦的同时主动 blur 当前获焦的 tooltip trigger，
      // 让窗口切回时焦点不再回到触发器上，从而避免误触发；
      // 选择器限定了仅作用于 tooltip trigger，不影响 input/textarea 等元素。
      const active = document.activeElement
      if (active instanceof HTMLElement && active.closest('[data-slot="tooltip-trigger"]')) {
        active.blur()
      }
    }
    window.addEventListener('blur', handleBlur)
    return () => window.removeEventListener('blur', handleBlur)
  }, [open, onOpenChange])

  return (
    <TooltipPrimitive.Root data-slot="tooltip" open={open} onOpenChange={onOpenChange} {...props} />
  )
}

function TooltipTrigger({ ...props }: React.ComponentProps<typeof TooltipPrimitive.Trigger>) {
  return <TooltipPrimitive.Trigger data-slot="tooltip-trigger" {...props} />
}

function TooltipContent({
  className,
  sideOffset = 0,
  children,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Content>) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        data-slot="tooltip-content"
        sideOffset={sideOffset}
        className={cn(
          'z-50 inline-flex w-fit max-w-xs origin-(--radix-tooltip-content-transform-origin) items-center gap-1.5 rounded-md bg-foreground px-3 py-1.5 text-xs text-background has-data-[slot=kbd]:pr-1.5 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 **:data-[slot=kbd]:relative **:data-[slot=kbd]:isolate **:data-[slot=kbd]:z-50 **:data-[slot=kbd]:rounded-sm data-[state=delayed-open]:animate-in data-[state=delayed-open]:fade-in-0 data-[state=delayed-open]:zoom-in-95 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95',
          className
        )}
        {...props}
      >
        {children}
        <TooltipPrimitive.Arrow className="z-50 size-2.5 translate-y-[calc(-50%_-_2px)] rotate-45 rounded-[2px] bg-foreground fill-foreground" />
      </TooltipPrimitive.Content>
    </TooltipPrimitive.Portal>
  )
}

export { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger }
