import { Clock, LayoutGrid, MonitorSmartphone, Star, type LucideIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import type { SearchTagOption } from '@/lib/search-tags'
import { cn } from '@/lib/utils'
import { buildCandidates, DIMENSION_LABEL_KEYS, type SourceOption } from './composite-search-model'

interface HistoryFilterPanelProps {
  contentFilter: Filter
  sourceFilter: string | null
  tagFilter: string | null
  timeRange: TimeRangePreset
  onContentFilterChange: (filter: Filter) => void
  onTagFilterChange: (tag: string | null) => void
  onSourceFilterChange: (id: string | null) => void
  onTimeRangeChange: (preset: TimeRangePreset) => void
  sourceOptions: SourceOption[]
  tagOptions: SearchTagOption[]
}

/** A single clickable filter row: icon + label with an active highlight. */
function PanelRow({
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
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={cn(
        'flex w-full items-center gap-2 rounded-[1.25rem] px-2.5 py-1.5 text-left text-[13px] transition-colors',
        active
          ? 'bg-muted/50 text-foreground ring-1 ring-border/50'
          : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
      )}
    >
      <Icon className={cn('size-3.5 shrink-0', active ? 'text-muted-foreground' : 'opacity-70')} />
      <span className="truncate">{label}</span>
    </button>
  )
}

/** A labeled group of rows; the header is omitted when `title` is empty. */
function PanelSection({ title, children }: { title?: string; children: React.ReactNode }) {
  return (
    <div className="mb-3">
      {title && (
        <div className="px-2 pb-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground/50">
          {title}
        </div>
      )}
      <div className="space-y-0.5">{children}</div>
    </div>
  )
}

/**
 * Vertical "organize" panel for the History page: the page-local filter controls
 * (quick views, content type, source device, time range) laid out as grouped
 * rows beside the card grid. It is a pure skin over the History page's filter
 * state — every selection dispatches through the `on*Change` props, the same
 * single source of truth the composite search box reads — so selecting here also
 * surfaces a chip in the search box and vice versa.
 *
 * Content type and the quick views (all / favorited) share one selection
 * register (`contentFilter`), so they are mutually exclusive radio rows; source
 * and time are independent dimensions with their own "no filter" reset row.
 */
function HistoryFilterPanel({
  contentFilter,
  sourceFilter,
  tagFilter,
  timeRange,
  onContentFilterChange,
  onTagFilterChange,
  onSourceFilterChange,
  onTimeRangeChange,
  sourceOptions,
  tagOptions,
}: HistoryFilterPanelProps) {
  const { t } = useTranslation()
  const current = { type: contentFilter, tag: tagFilter, source: sourceFilter, time: timeRange }
  const typeCandidates = buildCandidates('type', '', { t, sourceOptions, tagOptions, current })
  const tagCandidates = buildCandidates('tag', '', {
    t,
    sourceOptions,
    tagOptions,
    current,
  }).filter(opt => opt.value !== Filter.Favorited)
  const sourceCandidates = buildCandidates('source', '', { t, sourceOptions, tagOptions, current })
  const timeCandidates = buildCandidates('time', '', { t, sourceOptions, tagOptions, current })

  return (
    <aside className="no-scrollbar w-44 shrink-0 overflow-y-auto border-r border-border/60 bg-muted/20 px-2 py-3">
      {/* Quick views — share the content-type register, so radio-exclusive. */}
      <PanelSection>
        <PanelRow
          icon={LayoutGrid}
          label={t('history.filter.all')}
          active={contentFilter === Filter.All}
          onClick={() => onContentFilterChange(Filter.All)}
        />
        <PanelRow
          icon={Star}
          label={t('history.filter.favorited')}
          active={contentFilter === Filter.Favorited}
          onClick={() => onContentFilterChange(Filter.Favorited)}
        />
      </PanelSection>

      <PanelSection title={t(DIMENSION_LABEL_KEYS.type)}>
        {typeCandidates.map(opt => (
          <PanelRow
            key={opt.id}
            icon={opt.icon}
            label={opt.label}
            active={opt.isActive}
            onClick={() => onContentFilterChange(opt.value as Filter)}
          />
        ))}
      </PanelSection>

      <PanelSection title={t('history.composite.dimension.tag')}>
        {tagCandidates.map(opt => (
          <PanelRow
            key={opt.id}
            icon={opt.icon}
            label={opt.label}
            active={opt.isActive}
            // Unlike source/time, the tag group has no "all" row; clicking the
            // active tag toggles it back off so the filter can return to null.
            onClick={() => onTagFilterChange(opt.isActive ? null : opt.value)}
          />
        ))}
      </PanelSection>

      {sourceOptions.length > 0 && (
        <PanelSection title={t(DIMENSION_LABEL_KEYS.source)}>
          <PanelRow
            icon={MonitorSmartphone}
            label={t('history.source.all')}
            active={sourceFilter === null}
            onClick={() => onSourceFilterChange(null)}
          />
          {sourceCandidates.map(opt => (
            <PanelRow
              key={opt.id}
              icon={opt.icon}
              label={opt.label}
              active={opt.isActive}
              onClick={() => onSourceFilterChange(opt.value)}
            />
          ))}
        </PanelSection>
      )}

      <PanelSection title={t(DIMENSION_LABEL_KEYS.time)}>
        <PanelRow
          icon={Clock}
          label={t('history.timeRange.all_time')}
          active={timeRange === 'all_time'}
          onClick={() => onTimeRangeChange('all_time')}
        />
        {timeCandidates.map(opt => (
          <PanelRow
            key={opt.id}
            icon={opt.icon}
            label={opt.label}
            active={opt.isActive}
            onClick={() => onTimeRangeChange(opt.value as TimeRangePreset)}
          />
        ))}
      </PanelSection>
    </aside>
  )
}

export default HistoryFilterPanel
