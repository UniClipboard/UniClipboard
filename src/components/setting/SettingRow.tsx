import type { ReactNode } from 'react'
import { isExperimentalFeature } from '@/components/setting/experimental-features'
import { ExperimentalBadge } from '@/components/setting/ExperimentalBadge'
import { cn } from '@/lib/utils'

interface SettingRowProps {
  label?: string
  labelExtra?: ReactNode
  description?: string
  children?: ReactNode
  className?: string
  /**
   * Data-driven experimental marker. When the key is registered in
   * `experimental-features.ts`, an ExperimentalBadge is rendered next to the label.
   */
  experimentalKey?: string
}

export function SettingRow({
  label,
  labelExtra,
  description,
  children,
  className,
  experimentalKey,
}: SettingRowProps) {
  const showExperimental = isExperimentalFeature(experimentalKey)

  return (
    <div
      className={cn(
        'flex min-w-0 flex-wrap items-center justify-between gap-x-6 gap-y-2.5 px-1 py-3.5',
        className
      )}
    >
      {(label || description) && (
        <div className="flex min-w-0 flex-[1_1_14rem] flex-col gap-1">
          {label && (
            <div className="flex flex-wrap items-center gap-2">
              <h4 className="text-sm font-medium">{label}</h4>
              {showExperimental && <ExperimentalBadge />}
              {labelExtra}
            </div>
          )}
          {description && (
            <p className="text-xs text-muted-foreground leading-relaxed break-words">
              {description}
            </p>
          )}
        </div>
      )}
      {children && <div className="ml-auto max-w-full min-w-0 shrink-0">{children}</div>}
    </div>
  )
}
