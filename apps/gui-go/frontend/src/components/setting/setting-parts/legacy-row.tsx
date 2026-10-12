import { isExperimentalFeature } from '@/components/setting/experimental-features'
import { ExperimentalBadge } from '@/components/setting/ExperimentalBadge'
import type { SettingRowProps } from '@/components/setting/setting-parts/types'
import { cn } from '@/lib/utils'

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
              <span data-slot="setting-label" className="text-ui-body font-normal">
                {label}
              </span>
              {showExperimental && <ExperimentalBadge />}
              {labelExtra}
            </div>
          )}
          {description && (
            <p
              data-slot="setting-description"
              className="max-w-[34rem] text-ui-caption-relaxed text-muted-foreground break-words"
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
