import type { LucideIcon } from 'lucide-react'
import { cn } from '@/lib/utils'

export function DeviceTrustOutcomeRow({
  icon: Icon,
  label,
  devices,
  tone,
}: {
  icon: LucideIcon
  label: string
  devices: string
  tone: 'success' | 'danger'
}) {
  return (
    <span
      className={cn(
        'grid min-w-0 grid-cols-[1rem_5rem_minmax(0,1fr)] items-start gap-2 text-xs leading-5',
        tone === 'success' ? 'text-emerald-600 dark:text-emerald-400' : 'text-destructive'
      )}
    >
      <Icon className="mt-0.5 size-4" aria-hidden="true" />
      <span>{label}</span>
      <span className="break-words font-medium">{devices}</span>
    </span>
  )
}
