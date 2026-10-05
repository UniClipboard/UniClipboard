import { ChevronDown } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { type ChipData, DIMENSION_LABEL_KEYS, type Dimension } from './composite-search-model'

const FACETS: readonly Dimension[] = ['type', 'source', 'tag', 'time']

interface SearchFacetRowProps {
  chips: ChipData[]
  onSeedDimension: (dimension: Dimension) => void
  onClearAll: () => void
}

/** HList.dc.html facet row: one button per filter dimension, badged with how
 * many chips it holds; a button seeds the search field with that dimension. */
function SearchFacetRow({ chips, onSeedDimension, onClearAll }: SearchFacetRowProps) {
  const { t } = useTranslation()

  return (
    <div className="flex flex-1 items-center gap-2">
      {FACETS.map(dimension => {
        const count = chips.filter(chip => chip.dimension === dimension).length
        return (
          <button
            key={dimension}
            type="button"
            onMouseDown={event => event.preventDefault()}
            onClick={() => onSeedDimension(dimension)}
            className={cn(
              'inline-flex h-7.5 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-ui-body font-medium transition-colors',
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
          className="shrink-0 whitespace-nowrap text-ui-body font-medium text-history-accent hover:underline"
        >
          {t('history.composite.clearAll')}
        </button>
      )}
    </div>
  )
}

export default SearchFacetRow
