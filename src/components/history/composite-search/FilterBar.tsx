import { Check, ChevronDown, Clock, Laptop, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import {
  applyDimensionValue,
  buildCandidates,
  DIMENSION_DEFAULTS,
  DIMENSION_LABEL_KEYS,
  resetDimensionValue,
  type SourceOption,
} from './composite-search-model'

interface FilterBarProps {
  contentFilter: Filter
  sourceFilter: string | null
  timeRange: TimeRangePreset
  onContentFilterChange: (filter: Filter) => void
  onSourceFilterChange: (id: string | null) => void
  onTimeRangeChange: (preset: TimeRangePreset) => void
  sourceOptions: SourceOption[]
}

/** i18n keys for each dropdown dimension's "no filter" reset row. */
const RESET_LABEL_KEYS = {
  source: 'history.source.all',
  time: 'history.timeRange.all_time',
} as const

const PILL_BASE =
  'flex items-center gap-1 h-7 px-2.5 rounded-full text-[12px] font-medium whitespace-nowrap transition-colors'
const PILL_ACTIVE = 'bg-foreground/8 text-foreground'
const PILL_IDLE = 'text-muted-foreground/60 hover:bg-muted/40 hover:text-muted-foreground'

/**
 * Quick filter bar for mouse users, left of the composite search box. Content
 * types are flat one-click pills; source/time are dropdowns. Every control is a
 * view over the History page's filter state (same single source of truth as the
 * chips), so selecting here also surfaces a chip in the search box.
 */
function FilterBar({
  contentFilter,
  sourceFilter,
  timeRange,
  onContentFilterChange,
  onSourceFilterChange,
  onTimeRangeChange,
  sourceOptions,
}: FilterBarProps) {
  const { t } = useTranslation()
  const current = { type: contentFilter, source: sourceFilter, time: timeRange }
  const handlers = { onContentFilterChange, onSourceFilterChange, onTimeRangeChange }
  const typeOptions = buildCandidates('type', '', { t, sourceOptions, current })

  const toggleType = (value: string) => {
    if (contentFilter === value) onContentFilterChange(DIMENSION_DEFAULTS.type)
    else applyDimensionValue('type', value, handlers)
  }

  return (
    <div className="flex shrink-0 items-center gap-1.5">
      {typeOptions.map(opt => {
        const Icon = opt.icon
        return (
          <button
            key={opt.id}
            type="button"
            onClick={() => toggleType(opt.value)}
            aria-label={
              opt.isActive ? t('history.composite.removeFilter', { filter: opt.label }) : undefined
            }
            className={cn(
              PILL_BASE,
              opt.isActive ? PILL_ACTIVE : PILL_IDLE,
              opt.isActive && 'group relative'
            )}
          >
            <span
              className={cn(
                'flex items-center gap-1 transition-opacity',
                opt.isActive && 'group-hover:opacity-0'
              )}
            >
              <Icon className="size-3" />
              {opt.label}
            </span>
            {opt.isActive && (
              <span className="absolute inset-0 flex items-center justify-center opacity-0 transition-opacity group-hover:opacity-100">
                <X className="size-3.5" />
              </span>
            )}
          </button>
        )
      })}

      {(['source', 'time'] as const).map(dim => {
        if (dim === 'source' && sourceOptions.length === 0) return null
        const options = buildCandidates(dim, '', { t, sourceOptions, current })
        const active = dim === 'source' ? sourceFilter !== null : timeRange !== 'all_time'
        const TriggerIcon = dim === 'source' ? Laptop : Clock
        return (
          <DropdownMenu key={dim}>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                aria-label={t(DIMENSION_LABEL_KEYS[dim])}
                className={cn(PILL_BASE, active ? PILL_ACTIVE : PILL_IDLE)}
              >
                <TriggerIcon className="size-3" />
                {t(DIMENSION_LABEL_KEYS[dim])}
                <ChevronDown className="size-3 opacity-50" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-44">
              <DropdownMenuItem
                onClick={() => resetDimensionValue(dim, handlers)}
                className="flex items-center justify-between text-[12px]"
              >
                {t(RESET_LABEL_KEYS[dim])}
                {!active && <Check className="size-3 text-primary" />}
              </DropdownMenuItem>
              {options.map(opt => {
                const Icon = opt.icon
                return (
                  <DropdownMenuItem
                    key={opt.id}
                    onClick={() => applyDimensionValue(dim, opt.value, handlers)}
                    className="flex items-center justify-between gap-2 text-[12px]"
                  >
                    <span className="flex items-center gap-1.5 truncate">
                      <Icon className="size-3 shrink-0 opacity-60" />
                      <span className="truncate">{opt.label}</span>
                    </span>
                    {opt.isActive && <Check className="size-3 shrink-0 text-primary" />}
                  </DropdownMenuItem>
                )
              })}
            </DropdownMenuContent>
          </DropdownMenu>
        )
      })}
    </div>
  )
}

export default FilterBar
