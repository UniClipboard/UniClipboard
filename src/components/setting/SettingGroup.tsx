import { useId, type ReactNode } from 'react'
import { cn } from '@/lib/utils'

interface SettingGroupProps {
  title?: string
  children: ReactNode
  className?: string
}

export function SettingGroup({ title, children, className }: SettingGroupProps) {
  const titleId = useId()

  return (
    <section
      aria-labelledby={title ? titleId : undefined}
      className={cn('flex min-w-0 flex-col gap-1', className)}
    >
      {title && (
        <h3
          id={titleId}
          className="border-b border-border/60 px-1 pb-3 text-base font-semibold leading-6"
        >
          {title}
        </h3>
      )}
      <div className="min-w-0 divide-y divide-border/50 text-card-foreground">{children}</div>
    </section>
  )
}
