import { Inbox, MonitorSmartphone, Pin, Settings, Trash2, type LucideIcon } from 'lucide-react'
import type { ReactElement } from 'react'
import { useTranslation } from 'react-i18next'
import { NavLink } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { useWindowDragging } from '@/hooks/useWindowDragging'
import { cn } from '@/lib/utils'
import type { SidebarLibraryFilter } from './history-sidebar-types'
import { LibraryToggleButton } from './LibraryToggle'

interface HistorySidebarRailProps {
  context: 'history' | 'devices'
  libraryActive: (filter: Filter) => boolean
  onSelectLibrary: (filter: SidebarLibraryFilter) => void
  /** Hovering the toggle peeks the full sidebar over the content. */
  onTogglePointerEnter: () => void
  onTogglePointerLeave: () => void
}

function RailItem({
  icon: Icon,
  label,
  active = false,
  disabled = false,
  render,
}: {
  icon: LucideIcon
  label: string
  active?: boolean
  disabled?: boolean
  /** The interactive element; a button or a NavLink. */
  render: ReactElement<Record<string, unknown>>
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={render}
        aria-label={label}
        aria-current={active ? 'true' : undefined}
        aria-disabled={disabled || undefined}
        className={cn(
          'flex size-8.5 shrink-0 items-center justify-center rounded-lg text-sidebar-foreground transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/60',
          active ? 'bg-foreground/8' : !disabled && 'hover:bg-foreground/5',
          disabled && 'cursor-default opacity-60'
        )}
      >
        <Icon className="size-3.5 opacity-75" aria-hidden="true" />
      </TooltipTrigger>
      <TooltipContent side="right" sideOffset={6}>
        {label}
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * Collapsed form of the Library sidebar (macOS): a 52px icon column, a 34px
 * button and 9px on each side, under a 64px top band. The band carries the
 * traffic lights, level with the first content column's search row, so it
 * stays bare; the column's own surface (drawn by HistorySidebar) starts below
 * it, led by the sidebar toggle.
 */
function HistorySidebarRail({
  context,
  libraryActive,
  onSelectLibrary,
  onTogglePointerEnter,
  onTogglePointerLeave,
}: HistorySidebarRailProps) {
  const { t } = useTranslation()
  const windowDragging = useWindowDragging()

  return (
    <TooltipProvider>
      <aside
        data-sidebar="rail"
        className="flex h-full w-13 shrink-0 flex-col items-center text-sidebar-foreground"
      >
        {/* The traffic-light band; the window drags from it. */}
        <div data-tauri-drag-region {...windowDragging} className="h-16 w-full shrink-0" />
        <div className="mb-1 mt-3 flex h-8.5 shrink-0 items-center">
          <LibraryToggleButton
            onPointerEnter={onTogglePointerEnter}
            onPointerLeave={onTogglePointerLeave}
          />
        </div>
        <nav
          aria-label={t('history.sidebar.library')}
          className="flex min-h-0 flex-1 flex-col items-center gap-1 pb-3"
        >
          <RailItem
            icon={Inbox}
            label={t('history.sidebar.allItems')}
            active={libraryActive(Filter.All)}
            render={<button type="button" onClick={() => onSelectLibrary(Filter.All)} />}
          />
          <RailItem
            icon={Pin}
            label={t('history.sidebar.pinned')}
            active={libraryActive(Filter.Favorited)}
            render={<button type="button" onClick={() => onSelectLibrary(Filter.Favorited)} />}
          />
          <RailItem
            icon={Trash2}
            label={t('history.sidebar.recentlyDeleted')}
            disabled
            render={<button type="button" disabled />}
          />
          <span className="mb-2.5 mt-1 h-px w-7 bg-sidebar-border" aria-hidden="true" />
          <RailItem
            icon={MonitorSmartphone}
            label={t('history.sidebar.devices')}
            active={context === 'devices'}
            render={<NavLink to="/devices" />}
          />
          <span className="flex-1" />
          <RailItem
            icon={Settings}
            label={t('history.sidebar.settings')}
            render={<NavLink to="/settings" />}
          />
        </nav>
      </aside>
    </TooltipProvider>
  )
}

export default HistorySidebarRail
