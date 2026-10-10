import React from 'react'
import { WindowControls } from '@/components/WindowControls'
import { useMacTrafficLightPosition } from '@/hooks/useMacTrafficLightPosition'
import { usePlatform } from '@/hooks/usePlatform'
import { useWindowControls } from '@/hooks/useWindowControls'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { cn } from '@/lib/utils'

interface TitleBarProps {
  className?: string
  rightSlot?: React.ReactNode
}

interface TitleBarSectionProps {
  className?: string
  rightSlot?: React.ReactNode
}

type ContentToolbarProps = TitleBarSectionProps

// macOS 三色交通灯相对系统标准位置的偏移，屏幕坐标系：正 X 向右、正 Y 向下。
// 自绘 titlebar 高度 40pt vs 系统默认 28pt，按钮要向下挪一点才视觉居中；
// 同时整体往右挪让它远离 macOS 窗口圆角。宿主实现见
// `apps/gui-go` 的窗口外观适配。
const MAC_TRAFFIC_LIGHT_OFFSET = {
  x: 0,
  y: 4,
} as const

export const SidebarTitle = ({ className, rightSlot }: TitleBarSectionProps) => {
  const { isMac } = usePlatform()

  return (
    <div
      data-tauri-drag-region
      className={cn(
        'relative flex h-10 shrink-0 select-none items-center',
        isMac ? 'pl-18 pr-3' : 'px-3',
        className
      )}
    >
      {rightSlot && (
        <div className="relative z-10 flex items-center" data-tauri-drag-region="false">
          {rightSlot}
        </div>
      )}
    </div>
  )
}

export const ContentToolbar = ({ className, rightSlot }: ContentToolbarProps) => {
  const { hasCustomWindowControls } = useWindowFrame()
  const { toggleMaximize } = useWindowControls()
  useMacTrafficLightPosition(MAC_TRAFFIC_LIGHT_OFFSET)

  return (
    <div
      data-tauri-drag-region
      onDoubleClick={() => {
        if (!hasCustomWindowControls) return
        void toggleMaximize()
      }}
      className={cn(
        'relative z-20 flex h-10 w-full shrink-0 select-none items-center justify-end bg-transparent',
        className
      )}
    >
      {rightSlot && (
        <div
          className="relative z-10 flex min-w-0 flex-1 items-center px-3"
          data-tauri-drag-region="deep"
        >
          {rightSlot}
        </div>
      )}
      {hasCustomWindowControls && <WindowControls />}
    </div>
  )
}

export const TitleBar = ({ className, rightSlot }: TitleBarProps) => {
  return (
    <div
      data-tauri-drag-region
      className={cn('relative z-20 flex h-10 w-full shrink-0 bg-transparent', className)}
    >
      <SidebarTitle className="min-w-0 flex-1" />
      <ContentToolbar className="w-auto" rightSlot={rightSlot} />
    </div>
  )
}

export default TitleBar
