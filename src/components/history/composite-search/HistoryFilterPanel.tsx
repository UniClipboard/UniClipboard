import { getCurrentWindow } from '@tauri-apps/api/window'
import { ChevronDown, LayoutGrid, Star, type LucideIcon } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { useSidebarSlot } from '@/contexts/sidebar-slot-context'
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

/** A single clickable filter row: icon + label with an active highlight. */
function PanelRow({
  icon: Icon,
  label,
  active,
  collapsed,
  onClick,
}: {
  icon: LucideIcon
  label: string
  active: boolean
  collapsed: boolean
  onClick: () => void
}) {
  if (collapsed) {
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
                'flex size-8 items-center justify-center rounded-md transition-colors',
                active
                  ? 'bg-primary/15 text-primary ring-1 ring-inset ring-primary/30 dark:bg-primary/20'
                  : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
              )}
            />
          }
        >
          <Icon className="size-4" />
        </TooltipTrigger>
        <TooltipContent side="right" sideOffset={6}>
          {label}
        </TooltipContent>
      </Tooltip>
    )
  }

  return (
    <button
      data-tauri-drag-region="false"
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
function PanelSection({
  title,
  separated = false,
  children,
}: {
  title?: string
  separated?: boolean
  children: React.ReactNode
}) {
  const [expanded, setExpanded] = useState(true)

  return (
    <div className={cn('mb-3', separated && 'border-t border-border/35 pt-3')}>
      {title && (
        <button
          data-tauri-drag-region="false"
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded(value => !value)}
          className="flex h-7 w-full items-center justify-between rounded-md px-2 text-[11px] font-medium uppercase text-muted-foreground/50 transition-colors hover:bg-muted/50 hover:text-muted-foreground"
        >
          <span>{title}</span>
          <ChevronDown className={cn('size-3 transition-transform', !expanded && '-rotate-90')} />
        </button>
      )}
      {(!title || expanded) && <div className="space-y-0.5">{children}</div>}
    </div>
  )
}

/**
 * High-frequency History views and tags shown in the app sidebar. Advanced
 * dimensions stay in the composite search control instead of lengthening this
 * navigation surface.
 */
function HistoryFilterPanel({
  contentFilter,
  tagFilter,
  onContentFilterChange,
  onTagFilterChange,
  tagOptions,
}: HistoryFilterPanelProps) {
  const { t } = useTranslation()
  const { sidebarCollapsed } = useSidebarSlot()
  const dragStartRef = useRef<{ x: number; y: number } | null>(null)
  const current = {
    type: contentFilter,
    tag: tagFilter,
    source: null,
    time: 'all_time' as const,
    extension: null,
  }
  const tagCandidates = buildCandidates('tag', '', {
    t,
    sourceOptions: [],
    tagOptions,
    current,
  }).filter(opt => opt.value !== Filter.Favorited)

  return (
    <TooltipProvider delay={300}>
      <div
        data-tauri-drag-region
        onPointerDownCapture={event => {
          if (event.button !== 0) return
          dragStartRef.current = { x: event.clientX, y: event.clientY }
        }}
        onPointerMoveCapture={event => {
          const start = dragStartRef.current
          if (!start || (event.buttons & 1) === 0) return
          if (Math.hypot(event.clientX - start.x, event.clientY - start.y) < 4) return
          dragStartRef.current = null
          void getCurrentWindow()
            .startDragging()
            .catch(() => undefined)
        }}
        onPointerUpCapture={() => {
          dragStartRef.current = null
        }}
        className={cn(
          'flex h-full w-full flex-col py-3',
          sidebarCollapsed ? 'items-center px-2' : 'px-3'
        )}
      >
        <PanelSection>
          <PanelRow
            icon={LayoutGrid}
            label={t('history.filter.all')}
            active={contentFilter === Filter.All}
            collapsed={sidebarCollapsed}
            onClick={() => onContentFilterChange(Filter.All)}
          />
          <PanelRow
            icon={Star}
            label={t('history.filter.favorited')}
            active={contentFilter === Filter.Favorited}
            collapsed={sidebarCollapsed}
            onClick={() => onContentFilterChange(Filter.Favorited)}
          />
        </PanelSection>

        <div className="no-scrollbar min-h-0 flex-1 overflow-y-auto">
          <PanelSection
            title={sidebarCollapsed ? undefined : t('history.composite.dimension.tag')}
            separated={sidebarCollapsed}
          >
            {tagCandidates.map(opt => (
              <PanelRow
                key={opt.id}
                icon={opt.icon}
                label={opt.label}
                active={opt.isActive}
                collapsed={sidebarCollapsed}
                onClick={() => onTagFilterChange(opt.isActive ? null : opt.value)}
              />
            ))}
          </PanelSection>
        </div>
      </div>
    </TooltipProvider>
  )
}

export default HistoryFilterPanel
