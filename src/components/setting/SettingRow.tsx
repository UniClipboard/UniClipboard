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
      data-slot="setting-row"
      className={cn(
        'flex min-w-0 flex-wrap items-center justify-between gap-x-8 gap-y-3 px-1 py-4',
        className
      )}
    >
      {(label || description) && (
        <div className="flex min-w-0 flex-[1_1_14rem] flex-col gap-1">
          {label && (
            <div className="flex flex-wrap items-center gap-2">
              <span data-slot="setting-label" className="text-sm font-normal">
                {label}
              </span>
              {showExperimental && <ExperimentalBadge />}
              {labelExtra}
            </div>
          )}
          {description && (
            <p
              data-slot="setting-description"
              className="max-w-[34rem] text-xs text-muted-foreground leading-relaxed break-words"
            >
              {description}
            </p>
          )}
        </div>
      )}
      {children && <div className="ml-auto max-w-full min-w-0 shrink-0">{children}</div>}
    </div>
  )
}
