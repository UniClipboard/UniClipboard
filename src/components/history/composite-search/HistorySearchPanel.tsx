import { Check, Star, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import type { SearchTagOption } from '@/lib/search-tags'
import {
  applyDimensionValue,
  buildCandidates,
  buildChips,
  DIMENSION_LABEL_KEYS,
  resetDimensionValue,
  type Dimension,
  type DimensionHandlers,
  type SourceOption,
} from './composite-search-model'
import type { PanelOption } from './SuggestionPanel'

const DIMENSIONS: Dimension[] = ['type', 'tag', 'source', 'time', 'extension']

interface HistorySearchPanelProps extends DimensionHandlers {
  contentFilter: Filter
  sourceFilter: string | null
  tagFilter: string | null
  timeRange: TimeRangePreset
  extensionFilter: string | null
  sourceOptions: SourceOption[]
  tagOptions: SearchTagOption[]
  totalCount: number
  searchOptions: PanelOption[]
  searchHighlight: number
  searchSuggestionsOpen: boolean
  onSearchOptionSelect: (index: number) => void
  onSearchOptionHighlight: (index: number) => void
  onDismissSearchSuggestions: () => void
  onClearAll: () => void
  onClose: () => void
}

function HistorySearchPanel(props: HistorySearchPanelProps) {
  const { t } = useTranslation()
  const [dimension, setDimension] = useState<Dimension>('type')
  const current = useMemo(
    () => ({
      type: props.contentFilter,
      tag: props.tagFilter,
      source: props.sourceFilter,
      time: props.timeRange,
      extension: props.extensionFilter,
    }),
    [
      props.contentFilter,
      props.extensionFilter,
      props.sourceFilter,
      props.tagFilter,
      props.timeRange,
    ]
  )
  const handlers: DimensionHandlers = {
    onContentFilterChange: props.onContentFilterChange,
    onTagFilterChange: props.onTagFilterChange,
    onSourceFilterChange: props.onSourceFilterChange,
    onTimeRangeChange: props.onTimeRangeChange,
    onExtensionFilterChange: props.onExtensionFilterChange,
  }
  const candidateContext = {
    t,
    sourceOptions: props.sourceOptions,
    tagOptions: props.tagOptions,
    current,
  }
  const candidates = buildCandidates(dimension, '', candidateContext)
  const chips = buildChips(candidateContext)
  const visibleChips =
    props.contentFilter === Filter.Favorited
      ? [{ dimension: 'type' as const, label: t('history.filter.favorited'), icon: Star }, ...chips]
      : chips
  const typeIsAll = props.contentFilter === Filter.All

  return (
    <section
      aria-label={t('history.composite.title')}
      className="flex max-h-[26rem] min-h-0 w-full flex-col overflow-hidden rounded-lg border border-border bg-popover text-popover-foreground shadow-xl"
    >
      <div className="flex h-11 shrink-0 items-center gap-2 px-3">
        <span className="text-sm font-medium">{t('history.composite.title')}</span>
        <span className="shrink-0 text-xs text-muted-foreground">
          {t('history.composite.results', { count: props.totalCount })}
        </span>
        <button
          type="button"
          onClick={props.onClearAll}
          className="ml-auto h-7 shrink-0 rounded-md px-2 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
        >
          {t('history.composite.clearAll')}
        </button>
        <button
          type="button"
          aria-label={t('history.composite.close')}
          onClick={props.onClose}
          className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
        >
          <X className="size-4" />
        </button>
      </div>

      {visibleChips.length > 0 && (
        <div className="flex shrink-0 items-center gap-1.5 overflow-x-auto border-t border-border/60 px-3 py-2">
          {visibleChips.map(chip => {
            const Icon = chip.icon
            return (
              <span
                key={chip.dimension}
                className="flex h-7 shrink-0 items-center gap-1.5 rounded-md bg-muted px-2 text-xs text-foreground"
              >
                <Icon className="size-3.5 text-muted-foreground" />
                {chip.label}
                <button
                  type="button"
                  aria-label={t('history.composite.removeFilter', { filter: chip.label })}
                  onClick={() => resetDimensionValue(chip.dimension, handlers)}
                  className="flex size-4 items-center justify-center rounded-sm text-muted-foreground hover:bg-background hover:text-foreground"
                >
                  <X className="size-3" />
                </button>
              </span>
            )
          })}
        </div>
      )}

      <div className="flex h-56 min-h-0 shrink-0 border-t border-border">
        <nav
          aria-label={t('history.composite.filterCategories')}
          className="flex w-28 shrink-0 flex-col gap-1 overflow-y-auto border-r border-border p-2"
        >
          {DIMENSIONS.map(item => (
            <button
              key={item}
              type="button"
              onClick={() => {
                props.onDismissSearchSuggestions()
                setDimension(item)
              }}
              className={`flex h-8 shrink-0 items-center rounded-md px-2.5 text-left text-xs transition-colors ${
                dimension === item
                  ? 'bg-muted font-medium text-foreground'
                  : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
              }`}
            >
              {t(DIMENSION_LABEL_KEYS[item])}
            </button>
          ))}
        </nav>

        <div className="grid min-w-0 flex-1 grid-cols-1 content-start gap-1 overflow-y-auto p-2">
          {props.searchSuggestionsOpen ? (
            props.searchOptions.map((option, index) => {
              const Icon = option.icon
              const highlighted = index === props.searchHighlight
              return (
                <div key={option.id} className="min-w-0">
                  {option.header && (
                    <div className="px-3 pb-1 pt-2 text-[10px] font-medium uppercase text-muted-foreground/50">
                      {option.header}
                    </div>
                  )}
                  <button
                    id={option.id}
                    type="button"
                    role="option"
                    aria-selected={highlighted}
                    onMouseDown={event => event.preventDefault()}
                    onMouseEnter={() => props.onSearchOptionHighlight(index)}
                    onClick={() => props.onSearchOptionSelect(index)}
                    className={`flex h-9 w-full min-w-0 items-center gap-2 rounded-md px-3 text-left text-sm transition-colors ${
                      highlighted ? 'bg-muted text-foreground' : 'hover:bg-muted/60'
                    }`}
                  >
                    <Icon className="size-4 shrink-0 text-muted-foreground" />
                    <span className="min-w-0 flex-1 truncate">{option.label}</span>
                    {option.hint && (
                      <span className="shrink-0 font-mono text-[11px] text-muted-foreground/60">
                        {option.hint}
                      </span>
                    )}
                    {option.isActive && <Check className="size-4 shrink-0 text-primary" />}
                  </button>
                </div>
              )
            })
          ) : (
            <>
              {dimension === 'type' && (
                <button
                  type="button"
                  onClick={() => props.onContentFilterChange(Filter.All)}
                  className={`flex h-9 min-w-0 items-center gap-2 rounded-md px-3 text-left text-sm transition-colors ${
                    typeIsAll ? 'bg-primary/10 text-foreground' : 'hover:bg-muted'
                  }`}
                >
                  <span className="min-w-0 flex-1 truncate">{t('history.filter.all')}</span>
                  {typeIsAll && <Check className="size-4 shrink-0 text-primary" />}
                </button>
              )}
              {candidates.map(candidate => {
                const Icon = candidate.icon
                return (
                  <button
                    key={candidate.id}
                    type="button"
                    onClick={() =>
                      applyDimensionValue(candidate.dimension, candidate.value, handlers)
                    }
                    className={`flex h-9 min-w-0 items-center gap-2 rounded-md px-3 text-left text-sm transition-colors ${
                      candidate.isActive ? 'bg-primary/10 text-foreground' : 'hover:bg-muted'
                    }`}
                  >
                    <Icon className="size-4 shrink-0 text-muted-foreground" />
                    <span className="min-w-0 flex-1 truncate">{candidate.label}</span>
                    {candidate.isActive && <Check className="size-4 shrink-0 text-primary" />}
                  </button>
                )
              })}
              {candidates.length === 0 && dimension !== 'type' && (
                <p className="px-2 py-3 text-sm text-muted-foreground">
                  {t('history.composite.noMatches')}
                </p>
              )}
            </>
          )}
        </div>
      </div>
    </section>
  )
}

export default HistorySearchPanel
