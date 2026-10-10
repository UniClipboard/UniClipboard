import { ChevronDown } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { type ChipData, DIMENSION_LABEL_KEYS, type Dimension } from './composite-search-model'

const FACETS: readonly Dimension[] = ['type', 'source', 'tag', 'time']

/** A fixed 2px focus ring: the row scrolls horizontally, which also clips it
 * vertically, so the parent reserves exactly this much room above the buttons
 * (`pt-0.5` in HistoryPage) instead of the browser's own, larger outline. */
const FOCUS_RING = 'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50'

interface SearchFacetRowProps {
  chips: ChipData[]
  onSeedDimension: (dimension: Dimension) => void
  onClearAll: () => void
}

/** HList.dc.html facet row: one button per filter dimension, badged with how
 * many values it holds; a button seeds the search field with that dimension. */
function SearchFacetRow({ chips, onSeedDimension, onClearAll }: SearchFacetRowProps) {
  const { t } = useTranslation()

  return (
    <div className="flex flex-1 items-center gap-2">
      {FACETS.map(dimension => {
        const count = chips
          .filter(chip => chip.dimension === dimension)
          .reduce((sum, chip) => sum + chip.valueCount, 0)
        return (
          <button
            key={dimension}
            type="button"
            onMouseDown={event => event.preventDefault()}
            onClick={() => onSeedDimension(dimension)}
            className={cn(
              'inline-flex h-7.5 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-ui-body font-medium transition-colors',
              FOCUS_RING,
              count > 0
                ? 'border-history-accent-line bg-history-accent-soft text-foreground'
                : 'border-border bg-background text-foreground hover:bg-muted/60'
            )}
          >
            {t(DIMENSION_LABEL_KEYS[dimension])}
            {count > 0 && (
              <span className="inline-flex h-4.25 min-w-4.25 items-center justify-center rounded-full bg-history-accent px-1.25 text-ui-caption text-history-accent-foreground">
                {count}
              </span>
            )}
            <ChevronDown className="size-3 opacity-70" aria-hidden="true" />
          </button>
        )
      })}
      <span className="flex-1" />
      {chips.length > 0 && (
        <button
          type="button"
          onMouseDown={event => event.preventDefault()}
          onClick={onClearAll}
          className={cn(
            'shrink-0 rounded-sm whitespace-nowrap text-ui-body font-medium text-history-accent hover:underline',
            FOCUS_RING
          )}
        >
          {t('history.composite.clearAll')}
        </button>
      )}
    </div>
  )
}

export default SearchFacetRow
