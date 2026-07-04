import React from 'react'
import { cn } from '@/lib/utils'

export type StatusDotTone = 'success' | 'warning' | 'info' | 'off'

/**
 * Glowing presence dot used across the devices master-detail layout.
 * Tone encodes the connection channel: success = LAN direct, warning =
 * relay/degraded, info = mobile-sync activity, off = offline/idle.
 * The glow is built from the semantic theme tokens so it adapts to
 * light/dark and theme presets without per-tone hex values.
 */
const TONE_CLASSES: Record<StatusDotTone, string> = {
  success:
    'bg-success [box-shadow:0_0_0_2.5px_color-mix(in_oklch,var(--success)_20%,transparent),0_0_8px_color-mix(in_oklch,var(--success)_45%,transparent)]',
  warning:
    'bg-warning [box-shadow:0_0_0_2.5px_color-mix(in_oklch,var(--warning)_20%,transparent),0_0_8px_color-mix(in_oklch,var(--warning)_35%,transparent)]',
  info: 'bg-info [box-shadow:0_0_0_2.5px_color-mix(in_oklch,var(--info)_20%,transparent),0_0_8px_color-mix(in_oklch,var(--info)_35%,transparent)]',
  off: 'bg-muted-foreground/40',
}

const StatusDot: React.FC<{ tone: StatusDotTone; className?: string }> = ({ tone, className }) => (
  <span
    aria-hidden
    className={cn('size-[7px] shrink-0 rounded-full', TONE_CLASSES[tone], className)}
  />
)

export default StatusDot
