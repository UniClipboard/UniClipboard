import { useTranslation } from 'react-i18next'
import { QUICK_FILTER_ORDER } from '../constants'
import { Filter } from '@/api/clipboardItems'
import { cn } from '@/lib/utils'

interface QuickPanelTypeFilterBarProps {
  activeFilter: Filter
  onChange: (filter: Filter) => void
}

function QuickPanelTypeFilterBar({ activeFilter, onChange }: QuickPanelTypeFilterBarProps) {
  const { t } = useTranslation()

  return (
    <div className="flex flex-wrap items-center gap-1 px-3 pb-2" aria-label={t('history.composite.dimension.type')}>
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
              'rounded-md px-2 py-1 text-[11px] transition-colors',
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
