import React from 'react'
import { cn } from '@/lib/utils'

type InsetSurfaceProps = React.HTMLAttributes<HTMLDivElement>

/**
 * InsetSurface: A clean, minimal content container with a subtle 'elevated sheet' aesthetic.
 */
const InsetSurface: React.FC<InsetSurfaceProps> = ({ className, children, ...props }) => {
  return (
    <div
      className={cn(
        'relative flex min-h-0 flex-1 flex-col overflow-hidden transition-all duration-300',
        'rounded-[1.25rem] bg-card text-card-foreground border border-border/40',
        // Dark mode specific depth shadow, light mode is kept clean
        'shadow-none dark:shadow-[0_8px_30px_rgb(0,0,0,0.12)]',
        // Opaque surface on every platform: a full-viewport backdrop-blur here
        // pegged weak GPUs (Intel HD3000 / WebView2 on Windows). See issue #1129.
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
