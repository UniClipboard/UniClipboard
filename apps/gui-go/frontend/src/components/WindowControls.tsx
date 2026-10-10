import { Minus, Square, X } from 'lucide-react'
import type React from 'react'
import { useWindowControls } from '@/hooks/useWindowControls'
import { cn } from '@/lib/utils'

function WindowControlButton({
  onClick,
  children,
  className,
  'aria-label': ariaLabel,
}: {
  onClick: () => void
  children: React.ReactNode
  className?: string
  'aria-label': string
}) {
  return (
    <button
      type="button"
      aria-label={ariaLabel}
      data-tauri-drag-region="false"
      onClick={event => {
        event.stopPropagation()
        onClick()
      }}
      onDoubleClick={event => event.stopPropagation()}
      className={cn(
        'flex h-full w-12 items-center justify-center transition-colors duration-150',
        'text-muted-foreground hover:text-foreground',
        className
      )}
    >
      {children}
    </button>
  )
}

/**
 * Minimize / maximize / close buttons for the app-drawn window frame
 * (Windows and Linux). The caller decides whether the frame is app-drawn; this
 * component always renders the buttons. It fills the height of its parent.
 */
export function WindowControls({ className }: { className?: string }) {
  const { isMaximized, minimize, toggleMaximize, close } = useWindowControls()

  return (
    <div
      className={cn('relative z-10 flex h-full items-center', className)}
      data-tauri-drag-region="false"
    >
      <WindowControlButton aria-label="最小化" onClick={minimize}>
        <Minus className="size-4" />
      </WindowControlButton>
      <WindowControlButton aria-label={isMaximized ? '还原' : '最大化'} onClick={toggleMaximize}>
        <Square className="size-3.5" />
      </WindowControlButton>
      <WindowControlButton
        aria-label="关闭"
        onClick={close}
        className="hover:bg-red-500/90 hover:text-white"
      >
        <X className="size-4" />
      </WindowControlButton>
    </div>
  )
}

export default WindowControls
