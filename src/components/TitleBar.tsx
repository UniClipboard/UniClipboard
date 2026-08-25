import { getCurrentWindow } from '@tauri-apps/api/window'
import { Minus, Square, X } from 'lucide-react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { usePlatform } from '@/hooks/usePlatform'
import { useWindowFrame } from '@/hooks/useWindowFrame'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'

const log = createLogger('title-bar')

interface TitleBarProps {
  className?: string
  isSetupActive?: boolean
  rightSlot?: React.ReactNode
}

interface TitleBarSectionProps {
  className?: string
  rightSlot?: React.ReactNode
}

type ContentToolbarProps = TitleBarSectionProps

// macOS 三色交通灯相对系统标准位置的偏移，屏幕坐标系：正 X 向右、正 Y 向下。
// 自绘 titlebar 高度 40pt vs 系统默认 28pt，按钮要向下挪一点才视觉居中；
// 同时整体往右挪让它远离 macOS 窗口圆角。圆角本身由 tauri.conf.json
// `windowEffects.radius` 接管。后端实现见
// `crates/uc-tauri/src/commands/window_chrome.rs`。
const MAC_TRAFFIC_LIGHT_OFFSET = {
  x: 0,
  y: 4,
} as const

const TitleBarButton = ({
  onClick,
  children,
  className,
  'aria-label': ariaLabel,
}: {
  onClick: () => void
  children: React.ReactNode
  className?: string
  'aria-label': string
}) => (
  <button
    type="button"
    aria-label={ariaLabel}
    data-tauri-drag-region="false"
    onClick={e => {
      log.debug({ ariaLabel }, 'Button clicked')
      e.stopPropagation()
      onClick()
    }}
    onDoubleClick={event => event.stopPropagation()}
    className={cn(
      'h-full w-12 flex items-center justify-center transition-colors duration-150',
      'text-muted-foreground hover:text-foreground',
      className
    )}
  >
    {children}
  </button>
)

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
  const [isMaximized, setIsMaximized] = useState(false)

  const { isMac, isTauri } = usePlatform()
  const { hasCustomWindowControls } = useWindowFrame()
  const windowRef = useMemo(() => (isTauri ? getCurrentWindow() : null), [isTauri])

  const syncTrafficLightPosition = useCallback(() => {
    if (!isMac) return
    commands
      .setTrafficLightPosition(MAC_TRAFFIC_LIGHT_OFFSET.x, MAC_TRAFFIC_LIGHT_OFFSET.y)
      .catch(error => {
        log.error({ err: error }, 'Failed to set traffic light position')
      })
  }, [isMac])

  useEffect(() => {
    if (!isTauri || !windowRef) return

    let mounted = true

    // macOS 系统会在 unmaximize / 全屏切换后把按钮重置回标准位置，
    // mount 一次 + 每次 resize 都重发，保证视觉一致。
    syncTrafficLightPosition()

    windowRef.isMaximized().then(value => {
      if (mounted) setIsMaximized(value)
    })

    const unlistenPromise = windowRef.onResized(async () => {
      if (!mounted) return
      setIsMaximized(await windowRef.isMaximized())
      syncTrafficLightPosition()
    })

    return () => {
      mounted = false
      unlistenPromise.then(unlisten => unlisten())
    }
  }, [isTauri, windowRef, syncTrafficLightPosition])

  const handleMinimize = async () => {
    log.debug({ isTauri }, 'Minimize clicked')
    if (!isTauri || !windowRef) return
    try {
      log.debug('Calling minimize')
      await windowRef.minimize()
      log.debug('Minimize succeeded')
    } catch (error) {
      log.error({ err: error }, 'Minimize failed')
    }
  }

  const handleToggleMaximize = async () => {
    log.debug({ isTauri }, 'Toggle maximize clicked')
    if (!isTauri || !windowRef) return
    try {
      const maximized = await windowRef.isMaximized()
      log.debug({ maximized }, 'Current maximized state')
      if (maximized) {
        await windowRef.unmaximize()
      } else {
        await windowRef.maximize()
      }
      setIsMaximized(!maximized)
      log.debug('Toggle maximize succeeded')
    } catch (error) {
      log.error({ err: error }, 'Toggle maximize failed')
    }
  }

  const handleClose = async () => {
    log.debug({ isTauri }, 'Close clicked')
    if (!isTauri || !windowRef) return
    try {
      log.debug('Calling close')
      await windowRef.close()
      log.debug('Close succeeded')
    } catch (error) {
      log.error({ err: error }, 'Close failed')
    }
  }

  return (
    <div
      data-tauri-drag-region
      onDoubleClick={() => {
        if (!hasCustomWindowControls) return
        handleToggleMaximize()
      }}
      className={cn(
        'relative z-20 flex h-10 w-full shrink-0 select-none items-center justify-end bg-transparent',
        className
      )}
    >
      {rightSlot && (
        <div
          className="relative z-10 flex min-w-0 flex-1 items-center px-3"
          data-tauri-drag-region="false"
        >
          {rightSlot}
        </div>
      )}
      {hasCustomWindowControls && (
        <div className="relative z-10 mr-4 flex h-full items-center" data-tauri-drag-region="false">
          <TitleBarButton aria-label="最小化" onClick={handleMinimize}>
            <Minus className="size-4" />
          </TitleBarButton>
          <TitleBarButton
            aria-label={isMaximized ? '还原' : '最大化'}
            onClick={handleToggleMaximize}
          >
            <Square className="size-3.5" />
          </TitleBarButton>
          <TitleBarButton
            aria-label="关闭"
            onClick={handleClose}
            className="hover:bg-red-500/90 hover:text-white"
          >
            <X className="size-4" />
          </TitleBarButton>
        </div>
      )}
    </div>
  )
}

export const TitleBar = ({ className, isSetupActive = false, rightSlot }: TitleBarProps) => {
  if (isSetupActive) return null

  return (
    <div
      data-tauri-drag-region
      className={cn('relative z-20 flex h-10 w-full shrink-0 bg-transparent', className)}
    >
      <SidebarTitle className="min-w-0 flex-1" />
      <ContentToolbar rightSlot={rightSlot} />
    </div>
  )
}

export default TitleBar
