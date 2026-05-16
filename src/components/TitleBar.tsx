import { getCurrentWindow } from '@tauri-apps/api/window'
import { Minus, Square, X, Search } from 'lucide-react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation } from 'react-router-dom'
import { Input } from '@/components/ui/input'
import { usePlatform } from '@/hooks/usePlatform'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'

const log = createLogger('title-bar')

interface TitleBarProps {
  className?: string
  searchValue?: string
  onSearchChange?: (value: string) => void
  isSetupActive?: boolean
}

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

export const TitleBar = ({
  className,
  searchValue = '',
  onSearchChange,
  isSetupActive = false,
}: TitleBarProps) => {
  const [isMaximized, setIsMaximized] = useState(false)
  const location = useLocation()
  const { t } = useTranslation()

  // 使用 usePlatform hook 获取平台信息
  const { isWindows, isMac, isTauri } = usePlatform()
  const windowRef = useMemo(() => (isTauri ? getCurrentWindow() : null), [isTauri])

  // 检测是否在 Dashboard 页面
  const isDashboardPage = location.pathname === '/'
  // Setup 页面隐藏 TitleBar 保持沉浸感
  const shouldHideTitleBar = isSetupActive

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

  const [isSearchFocused, setIsSearchFocused] = useState(false)

  if (shouldHideTitleBar) {
    return null
  }

  return (
    <div
      data-tauri-drag-region
      className={cn(
        // Window chrome layer - sits in normal document flow (not fixed)
        // No z-index needed - proper layering via DOM hierarchy
        'h-10 w-full flex-shrink-0 select-none',
        'relative z-20 bg-transparent',
        className
      )}
    >
      <div
        data-tauri-drag-region
        className="relative z-10 h-full flex items-center justify-between cursor-default"
      >
        <div
          data-tauri-drag-region
          className={cn(
            'relative flex-1 flex items-center',
            'pr-4',
            // On macOS, add left padding to avoid traffic lights
            // On other platforms, use default padding
            isMac
              ? 'pl-16'
              : 'px-3' /* MVP: justify-center removed - restore with: , isDashboardPage ? 'justify-center' : '' */
          )}
        >
          <button
            type="button"
            data-tauri-drag-region
            aria-label="Toggle window maximize"
            className="absolute inset-0 z-0 cursor-default bg-transparent"
            onDoubleClick={() => {
              if (!isWindows) return
              handleToggleMaximize()
            }}
            onKeyDown={event => {
              if (!isWindows) return
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault()
                handleToggleMaximize()
              }
            }}
            tabIndex={-1}
          />
          {isDashboardPage ? (
            <div
              className={cn(
                'relative z-10 flex items-center w-64 max-w-xs',
                'transition-all duration-200',
                'opacity-0 pointer-events-none' /* MVP: search hidden - remove this line to restore */
              )}
            >
              <Search
                className={cn(
                  'absolute left-2.5 h-3.5 w-3.5 transition-colors duration-200',
                  isSearchFocused ? 'text-primary' : 'text-muted-foreground'
                )}
              />
              <Input
                data-tauri-drag-region="false"
                type="text"
                value={searchValue}
                onChange={e => onSearchChange?.(e.target.value)}
                placeholder={t('header.searchPlaceholder')}
                className={cn(
                  'h-7 w-full pl-8 pr-2.5 py-1',
                  'bg-muted/50 hover:bg-muted/70',
                  'border border-border/50 rounded-lg text-sm',
                  'focus-visible:bg-background focus-visible:border-primary/50',
                  'transition-all duration-200',
                  'focus-visible:ring-0 focus-visible:ring-offset-0',
                  'placeholder:text-muted-foreground/50'
                )}
                onFocus={() => setIsSearchFocused(true)}
                onBlur={() => setIsSearchFocused(false)}
              />
            </div>
          ) : null}
        </div>
        {isWindows && (
          <div className="flex items-center h-full bg-transparent" data-tauri-drag-region="false">
            <TitleBarButton aria-label="最小化" onClick={handleMinimize}>
              <Minus className="h-4 w-4" />
            </TitleBarButton>
            <TitleBarButton
              aria-label={isMaximized ? '还原' : '最大化'}
              onClick={handleToggleMaximize}
            >
              <Square className="h-3.5 w-3.5" />
            </TitleBarButton>
            <TitleBarButton
              aria-label="关闭"
              onClick={handleClose}
              className="hover:bg-red-500/90 hover:text-white"
            >
              <X className="h-4 w-4" />
            </TitleBarButton>
          </div>
        )}
      </div>
    </div>
  )
}

export default TitleBar
