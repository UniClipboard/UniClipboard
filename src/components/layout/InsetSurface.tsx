import React from 'react'
import { usePlatform } from '@/hooks/usePlatform'
import { cn } from '@/lib/utils'

type InsetSurfaceProps = React.HTMLAttributes<HTMLDivElement>

const InsetSurface: React.FC<InsetSurfaceProps> = ({ className, children, ...props }) => {
  const { isWindows } = usePlatform()

  return (
    <div
      className={cn(
        'relative flex min-h-0 flex-1 flex-col overflow-hidden',
        isWindows && 'rounded-tl-[22px] bg-background/92 backdrop-blur-sm',
        className
      )}
      {...props}
    >
      {isWindows && (
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 rounded-tl-[22px] ring-1 ring-white/12 ring-inset dark:ring-white/10"
        />
      )}
      <div className="relative z-10 flex min-h-0 flex-1 flex-col">{children}</div>
    </div>
  )
}

export default InsetSurface
