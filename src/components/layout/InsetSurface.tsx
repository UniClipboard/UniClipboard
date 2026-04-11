import React from 'react'
import { usePlatform } from '@/hooks/usePlatform'
import { cn } from '@/lib/utils'

type InsetSurfaceProps = React.HTMLAttributes<HTMLDivElement>

/**
 * InsetSurface: A clean, minimal content container with a subtle 'elevated sheet' aesthetic.
 */
const InsetSurface: React.FC<InsetSurfaceProps> = ({ className, children, ...props }) => {
  const { isWindows } = usePlatform()

  return (
    <div
      className={cn(
        'relative flex min-h-0 flex-1 flex-col overflow-hidden transition-all duration-300',
        'rounded-[1.25rem] bg-card text-card-foreground border border-border/40',
        // Dark mode specific depth shadow, light mode is kept clean
        'shadow-none dark:shadow-[0_8px_30px_rgb(0,0,0,0.12)]',
        // Platform specific material optimizations
        isWindows && 'bg-card/90 backdrop-blur-xl',
        className
      )}
      {...props}
    >
      {/* Content Area */}
      <div className="relative flex min-h-0 flex-1 flex-col">{children}</div>
    </div>
  )
}

export default InsetSurface
