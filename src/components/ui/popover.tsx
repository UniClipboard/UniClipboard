import * as PopoverPrimitive from '@radix-ui/react-popover'
import * as React from 'react'
import { cn } from '@/lib/utils'

const Popover = PopoverPrimitive.Root
const PopoverTrigger = PopoverPrimitive.Trigger
const PopoverAnchor = PopoverPrimitive.Anchor

// 桌面应用默认阻止 Radix 在关闭时把焦点还回 trigger。
// 为什么:这是个 Tauri WebView 应用,鼠标 hover/click 关闭弹窗后焦点回到
// trigger 会被 WebKit 的 :focus-visible 启发式命中,继续渲染 ring,看起来
// 像一圈残留的 border。桌面端用户对键盘焦点回流无感知诉求,而鼠标流程下
// 的视觉残留体感很差。键盘 tab 进入 trigger 时仍正常显示 ring (那条路径
// 不经过 close → returnFocus),无障碍未被破坏。
// 调用方仍可显式传 `onCloseAutoFocus` 覆盖此默认 (例如一个必须把焦点送回
// 表单字段的下拉)。
const defaultCloseAutoFocus = (event: Event) => {
  event.preventDefault()
}

const PopoverContent = React.forwardRef<
  React.ElementRef<typeof PopoverPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Content>
>(({ className, align = 'center', sideOffset = 8, onCloseAutoFocus, ...props }, ref) => (
  <PopoverPrimitive.Portal>
    <PopoverPrimitive.Content
      ref={ref}
      align={align}
      sideOffset={sideOffset}
      onCloseAutoFocus={onCloseAutoFocus ?? defaultCloseAutoFocus}
      className={cn(
        'z-50 w-72 rounded-xl border bg-popover p-3 text-popover-foreground shadow-lg ring-1 ring-foreground/10 outline-none',
        'data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
        'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2',
        className
      )}
      {...props}
    />
  </PopoverPrimitive.Portal>
))

PopoverContent.displayName = PopoverPrimitive.Content.displayName

export { Popover, PopoverTrigger, PopoverContent, PopoverAnchor }
