import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'

interface SettingGroupProps {
  title?: string
  children: ReactNode
  className?: string
}

export function SettingGroup({ title, children, className }: SettingGroupProps) {
  return (
    <fieldset data-slot="setting-group" className={cn('min-w-0', className)}>
      {title && (
        <legend data-slot="setting-group-title" className="mb-4 px-1 text-base font-semibold">
          {title}
        </legend>
      )}
      <div className="min-w-0 divide-y divide-border/25 text-card-foreground">{children}</div>
    </fieldset>
  )
}
