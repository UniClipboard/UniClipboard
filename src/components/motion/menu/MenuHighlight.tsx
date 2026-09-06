import { m, useReducedMotion } from 'framer-motion'
import { SPRING_LAYOUT } from '@/lib/ease'
import { cn } from '@/lib/utils'

export function MenuHighlight({
  layoutId,
  destructive = false,
}: {
  layoutId: string
  destructive?: boolean
}) {
  const reduce = useReducedMotion()
  return (
    <m.span
      layoutId={layoutId}
      className={cn(
        'pointer-events-none absolute inset-0 -z-10 rounded-lg',
        destructive ? 'bg-destructive/10' : 'bg-foreground/[0.065]'
      )}
      transition={reduce ? { duration: 0 } : SPRING_LAYOUT}
    />
  )
}
