import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { Dimension } from './composite-search-model'
import type { Relaxation } from './useZeroResultRelaxations'

interface ZeroResultRelaxationsProps {
  relaxations: Relaxation[]
  onRemove: (dimension: Dimension) => void
}

/** Empty-state offers: drop one filter, with the result count it would give. */
function ZeroResultRelaxations({ relaxations, onRemove }: ZeroResultRelaxationsProps) {
  const { t } = useTranslation()
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
