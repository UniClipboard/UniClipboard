import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import { cn } from '@/lib/utils'
import { QUICK_FILTER_ORDER, QUICK_PANEL_GUTTER_CLASS_NAME } from '@/quick-panel/constants'

interface QuickPanelTypeFilterBarProps {
  activeFilter: Filter
  onChange: (filter: Filter) => void
}

function QuickPanelTypeFilterBar({ activeFilter, onChange }: QuickPanelTypeFilterBarProps) {
  const { t } = useTranslation()

  return (
    <div
      className={cn(
        QUICK_PANEL_GUTTER_CLASS_NAME,
        'flex shrink-0 items-center gap-0.5 overflow-x-auto pb-1.5'
      )}
      aria-label={t('history.composite.dimension.type')}
    >
      {QUICK_FILTER_ORDER.map(filter => {
        const active = activeFilter === filter
        const label = filter === Filter.All ? t('history.filter.all') : t(`history.type.${filter}`)

        return (
          <button
            key={filter}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(filter)}
            className={cn(
              'inline-flex h-6 shrink-0 items-center whitespace-nowrap rounded-md px-1.5 text-[11px] transition-colors',
              active
                ? 'bg-primary text-primary-foreground'
                : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground'
            )}
          >
            {label}
          </button>
        )
      })}
    </div>
  )
}

export default QuickPanelTypeFilterBar
