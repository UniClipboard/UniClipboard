import React from 'react'
import { cn } from '@/lib/utils'

export type StatusDotTone = 'success' | 'warning' | 'info' | 'off'

/** A quiet status indicator paired with a readable status label. */
const TONE_CLASSES: Record<StatusDotTone, string> = {
  success: 'bg-success',
  warning: 'bg-warning',
  info: 'bg-info',
  off: 'bg-muted-foreground/40',
}

const StatusDot: React.FC<{ tone: StatusDotTone; className?: string }> = ({ tone, className }) => (
  <span
    aria-hidden
    className={cn('size-1.5 shrink-0 rounded-full', TONE_CLASSES[tone], className)}
  />
)

export default StatusDot
