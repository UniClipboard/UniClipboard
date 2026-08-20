import { LayoutGrid, Star, X, type LucideIcon } from 'lucide-react'
import type { WheelEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import type { SearchTagOption } from '@/lib/search-tags'
import { cn } from '@/lib/utils'
import { buildCandidates, type SourceOption } from './composite-search-model'

interface HistoryFilterPanelProps {
  contentFilter: Filter
  sourceFilter: string | null
  tagFilter: string | null
  timeRange: TimeRangePreset
  extensionFilter: string | null
  onContentFilterChange: (filter: Filter) => void
  onTagFilterChange: (tag: string | null) => void
  onSourceFilterChange: (id: string | null) => void
  onTimeRangeChange: (preset: TimeRangePreset) => void
  onExtensionFilterChange: (extension: string | null) => void
  sourceOptions: SourceOption[]
  tagOptions: SearchTagOption[]
}

function FilterButton({
  icon: Icon,
  label,
  active,
  onClick,
}: {
  icon: LucideIcon
  label: string
  active: boolean
  onClick: () => void
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            data-tauri-drag-region="false"
            type="button"
            onClick={onClick}
            aria-label={label}
            aria-pressed={active}
            className={cn(
              'flex size-7 shrink-0 items-center justify-center rounded-full transition-colors',
              active
                ? 'bg-muted/50 text-foreground'
                : 'text-muted-foreground hover:bg-muted/35 hover:text-foreground'
            )}
          />
        }
      >
        <Icon className={cn('size-3.5 shrink-0', active ? 'opacity-80' : 'opacity-65')} />
      </TooltipTrigger>
      <TooltipContent side="bottom" sideOffset={6}>
        {label}
      </TooltipContent>
    </Tooltip>
  )
}

function HistoryFilterPanel({
  contentFilter,
  sourceFilter,
  tagFilter,
  timeRange,
  extensionFilter,
  onContentFilterChange,
  onTagFilterChange,
  onSourceFilterChange,
  onTimeRangeChange,
  onExtensionFilterChange,
  sourceOptions,
  tagOptions,
}: HistoryFilterPanelProps) {
  const { t } = useTranslation()
  const hasActiveFilter =
    contentFilter !== Filter.All ||
    sourceFilter !== null ||
    tagFilter !== null ||
    timeRange !== 'all_time' ||
    extensionFilter !== null
  const current = {
    type: contentFilter,
    tag: tagFilter,
    source: sourceFilter,
    time: timeRange,
    extension: extensionFilter,
  }
  const tagCandidates = buildCandidates('tag', '', {
    t,
    sourceOptions,
    tagOptions,
    current,
  }).filter(option => option.value !== Filter.Favorited)

  const handleWheel = (event: WheelEvent<HTMLDivElement>) => {
    if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return
    event.preventDefault()
    event.currentTarget.scrollLeft += event.deltaY
  }

  const clearFilters = () => {
    onContentFilterChange(Filter.All)
    onTagFilterChange(null)
    onSourceFilterChange(null)
    onTimeRangeChange('all_time')
    onExtensionFilterChange(null)
  }

  return (
    <TooltipProvider delay={300}>
      <div
        data-testid="history-filter-strip"
        aria-label={t('history.composite.filterCategories')}
        onWheel={handleWheel}
        className="no-scrollbar flex h-8 w-fit max-w-72 shrink-0 items-center gap-0.5 overflow-x-auto overscroll-x-contain rounded-full border border-border/25 bg-muted/15 p-0.5"
      >
        <FilterButton
          icon={hasActiveFilter ? X : LayoutGrid}
          label={t(hasActiveFilter ? 'history.composite.clearAll' : 'history.filter.all')}
          active={!hasActiveFilter}
          onClick={hasActiveFilter ? clearFilters : () => onContentFilterChange(Filter.All)}
        />
        <FilterButton
          icon={Star}
          label={t('history.filter.favorited')}
          active={contentFilter === Filter.Favorited}
          onClick={() => onContentFilterChange(Filter.Favorited)}
        />
        {tagCandidates.map(option => (
          <FilterButton
            key={option.id}
            icon={option.icon}
            label={option.label}
            active={option.isActive}
            onClick={() => onTagFilterChange(option.isActive ? null : option.value)}
          />
        ))}
      </div>
    </TooltipProvider>
  )
}

export default HistoryFilterPanel
