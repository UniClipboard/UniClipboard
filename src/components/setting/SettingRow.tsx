import type { ReactNode } from 'react'
import { isExperimentalFeature } from './experimental-features'
import { ExperimentalBadge } from './ExperimentalBadge'
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
    <div className={cn('flex items-center justify-between gap-4 px-4 py-3', className)}>
      {(label || description) && (
        <div className="space-y-0.5 min-w-0 flex-1">
          {label && (
            <div className="flex items-center gap-2">
              <h4 className="text-sm font-medium">{label}</h4>
              {showExperimental && <ExperimentalBadge />}
              {labelExtra}
            </div>
          )}
          {description && (
            <p className="text-xs text-muted-foreground leading-snug">{description}</p>
          )}
        </div>
      )}
      {children && <div className="shrink-0">{children}</div>}
    </div>
  )
}
