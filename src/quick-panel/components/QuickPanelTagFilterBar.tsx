import { Hash } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { SearchTagOption } from '@/lib/search-tags'
import { cn } from '@/lib/utils'

interface QuickPanelTagFilterBarProps {
  tagFilter: string | null
  tagOptions: SearchTagOption[]
  onChange: (tag: string | null) => void
}

function QuickPanelTagFilterBar({ tagFilter, tagOptions, onChange }: QuickPanelTagFilterBarProps) {
  const { t } = useTranslation()

  return (
    <div className="flex min-w-0 items-center gap-2 border-t border-border/50 bg-muted/5 px-3 py-1.5 text-[11px] text-muted-foreground">
      <span className="shrink-0">{t('history.composite.dimension.tag')}</span>
      <div
        className="scrollbar-thin flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
        data-testid="quick-panel-tag-filter-list"
      >
        {tagOptions.map(tag => {
          const active = tagFilter === tag.id
          const label = t(`history.type.${tag.id}`, { defaultValue: tag.id })

          return (
            <button
              key={tag.id}
              type="button"
              aria-pressed={active}
              onClick={() => onChange(active ? null : tag.id)}
              className={cn(
                'inline-flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-[11px] transition-colors',
                active
                  ? 'bg-primary text-primary-foreground'
                  : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground'
              )}
            >
              <Hash className="size-3" />
              {label}
            </button>
          )
        })}
      </div>
    </div>
  )
}

export default QuickPanelTagFilterBar
