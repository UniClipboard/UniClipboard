import { m, useReducedMotion } from 'framer-motion'
import { isLowEffectsEnabled } from '@/lib/platform'
import { cn } from '@/lib/utils'

interface SelectionIndicatorProps {
  layoutId: string
  className?: string
}

export default function SelectionIndicator({ layoutId, className }: SelectionIndicatorProps) {
  const reducedMotion = useReducedMotion()
  const instant = reducedMotion || isLowEffectsEnabled()

  return (
    <m.span
      aria-hidden="true"
      layoutId={layoutId}
      layout="position"
      className={cn('pointer-events-none absolute inset-0 rounded-[inherit]', className)}
      transition={
        instant ? { duration: 0 } : { type: 'spring', stiffness: 170, damping: 24, mass: 1.2 }
      }
    />
  )
}
