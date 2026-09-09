import { Hash } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { SearchTagOption } from '@/lib/search-tags'
import { cn } from '@/lib/utils'
import { QUICK_PANEL_FOOTER_CLASS_NAME } from '@/quick-panel/constants'

interface QuickPanelTagFilterBarProps {
  tagFilter: string | null
  tagOptions: SearchTagOption[]
  onChange: (tag: string | null) => void
}

function QuickPanelTagFilterBar({ tagFilter, tagOptions, onChange }: QuickPanelTagFilterBarProps) {
  const { t } = useTranslation()

  return (
    <div className={cn(QUICK_PANEL_FOOTER_CLASS_NAME, 'min-w-0 gap-2 bg-muted/5')}>
      <span className="shrink-0">{t('history.composite.dimension.tag')}</span>
      <div
        className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
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
