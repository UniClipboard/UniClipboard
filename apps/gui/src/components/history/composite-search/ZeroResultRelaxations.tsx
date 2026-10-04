import { X } from 'lucide-react'
import { Trans, useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import type { Dimension } from './composite-search-model'
import type { Relaxation } from './useZeroResultRelaxations'

interface ZeroResultRelaxationsProps {
  relaxations: Relaxation[]
  onRemove: (dimension: Dimension) => void
  /** `list`: HList.dc.html empty state — one full-width "Without …" row per
   * filter, under a title the page supplies. */
  variant?: 'compact' | 'list'
}

/** Empty-state offers: drop one filter, with the result count it would give. */
function ZeroResultRelaxations({
  relaxations,
  onRemove,
  variant = 'compact',
}: ZeroResultRelaxationsProps) {
  const { t } = useTranslation()
  if (variant === 'list') {
    return (
      <div className="mx-auto flex w-80 max-w-full flex-col gap-2 text-left">
        {relaxations.map(({ chip, count }) => (
          <button
            key={chip.dimension}
            type="button"
            disabled={count === 0}
            onClick={() => onRemove(chip.dimension)}
            aria-label={t('history.composite.removeFilter', { filter: chip.label })}
            className="flex h-10 items-center justify-between gap-3 rounded-lg border border-border bg-background px-3.5 text-ui-body text-foreground transition-colors hover:bg-muted/50 disabled:pointer-events-none"
          >
            <span className="min-w-0 truncate">
              <Trans
                t={t}
                i18nKey="history.composite.relaxWithout"
                values={{ filter: chip.label }}
                components={{ strong: <strong className="font-semibold" /> }}
              />
            </span>
            <span
              className={cn(
                'shrink-0',
                count > 0 ? 'font-semibold text-success' : 'text-muted-foreground'
              )}
            >
              {t('history.subtitle', { count })}
            </span>
          </button>
        ))}
      </div>
    )
  }
  return (
    <div className="flex flex-col items-center gap-2">
      <p className="text-ui-caption text-muted-foreground/60">
        {t('history.composite.relaxTitle')}
      </p>
      <div className="flex flex-wrap justify-center gap-1.5">
        {relaxations.map(({ chip, count }) => {
          const Icon = chip.icon
          return (
            <button
              key={chip.dimension}
              type="button"
              disabled={count === 0}
              onClick={() => onRemove(chip.dimension)}
              aria-label={t('history.composite.removeFilter', { filter: chip.label })}
              className="inline-flex h-7 items-center gap-1.5 rounded-full border border-border/60 bg-muted/60 px-2.5 text-ui-body text-foreground transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-40"
            >
              <X className="size-3 text-muted-foreground" />
              <Icon className="size-3.5 text-muted-foreground" />
              <span>{chip.label}</span>
              <span className="tabular-nums text-muted-foreground">
                {t('history.composite.results', { count })}
              </span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

export default ZeroResultRelaxations
