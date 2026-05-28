/**
 * Hover-driven popover primitive —— Radix HoverCard 的项目级包装。
 *
 * 为什么需要这个组件:
 * Popover 走 click;鼠标从 trigger 经"空隙"移入 content 期间会触发
 * close → open,导致弹窗闪烁。HoverCard 原生为 hover 场景设计,自带
 * openDelay / closeDelay,trigger 与 content 互相 hover 时不会进入
 * 关闭流程,正好解决这个问题。HoverCard 本身不接管焦点,也就不会出现
 * Popover 那种关闭后焦点还回 trigger 导致 `:focus-visible` 残留 ring
 * 的桌面端问题。
 *
 * 默认值:
 * - `openDelay = 80ms`:鼠标停留即可弹出,不至于划过 trigger 就闪一下
 * - `closeDelay = 120ms`:覆盖 trigger ↔ content 间的空隙跨越时间
 */

import * as HoverCardPrimitive from '@radix-ui/react-hover-card'
import * as React from 'react'
import { cn } from '@/lib/utils'

interface HoverCardRootProps extends React.ComponentPropsWithoutRef<
  typeof HoverCardPrimitive.Root
> {
  children?: React.ReactNode
}

function HoverCard({ openDelay = 80, closeDelay = 120, ...props }: HoverCardRootProps) {
  return <HoverCardPrimitive.Root openDelay={openDelay} closeDelay={closeDelay} {...props} />
}

const HoverCardTrigger = HoverCardPrimitive.Trigger

function HoverCardContent({
  ref,
  className,
  align = 'center',
  sideOffset = 8,
  style,
  onMouseDown,
  ...props
}: React.ComponentPropsWithoutRef<typeof HoverCardPrimitive.Content> & {
  ref?: React.Ref<React.ComponentRef<typeof HoverCardPrimitive.Content>>
}) {
  return (
    <HoverCardPrimitive.Portal>
      <HoverCardPrimitive.Content
        ref={ref}
        align={align}
        sideOffset={sideOffset}
        // HoverCard 走桌面 UI "默认不可框选" 约定 (globals.css#L428):
        // 这是一个 hover 详情面板,不是剪贴板正文,被拖蓝会让 webview 看起来
        // 像浏览器。
        //
        // 为什么用 inline style + onMouseDown 双保险,而不是 Tailwind class:
        // 1. 实测 `select-none` class 在某些上下文下未生效 (可能 JIT 漏扫
        //    或 cascade 被 ancestor 打破),用 inline style 直绕。
        // 2. webkit 的 `user-select: none` 不阻止从外部 drag 进来扩展选区,
        //    `onMouseDown.preventDefault()` 在 content 内启动 drag 时直接
        //    阻止选区起点,再加上 user-select: none 防止外部 drag 扩展进来
        //    时高亮 content 内文字。
        style={{ userSelect: 'none', WebkitUserSelect: 'none', ...style }}
        onMouseDown={e => {
          // 只在不是表单/可输入控件上抑制,避免误伤未来可能放进 hover 卡的
          // 复制按钮 (click 仍可触发,因 click 走 mouseup→click 链,
          // preventDefault on mousedown 不阻断 click)。
          const target = e.target as HTMLElement
          if (!target.closest('input, textarea, [contenteditable=""], [contenteditable="true"]')) {
            e.preventDefault()
          }
          onMouseDown?.(e)
        }}
        className={cn(
          'z-50 w-72 rounded-xl border bg-popover p-3 text-popover-foreground shadow-lg ring-1 ring-foreground/10 outline-none',
          'data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
          'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2',
          className
        )}
        {...props}
      />
    </HoverCardPrimitive.Portal>
  )
}

export { HoverCard, HoverCardTrigger, HoverCardContent }
