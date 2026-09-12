import { ChevronDown, ChevronUp, Hash } from 'lucide-react'
import { useLayoutEffect, useId, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { SearchTagOption } from '@/lib/search-tags'
import { cn } from '@/lib/utils'
import { QUICK_PANEL_FOOTER_CLASS_NAME } from '@/quick-panel/constants'

interface QuickPanelTagFilterBarProps {
  /** Comma-separated tags use the search API's OR semantics. */
  tagFilter: string | null
  tagOptions: SearchTagOption[]
  onChange: (tag: string | null) => void
}

function QuickPanelTagFilterBar({ tagFilter, tagOptions, onChange }: QuickPanelTagFilterBarProps) {
  const { t, i18n } = useTranslation()
  const listId = useId()
  const listRef = useRef<HTMLDivElement>(null)
  const [overflowing, setOverflowing] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const selectedTags = new Set(tagFilter?.split(',').filter(Boolean) ?? [])
  const showExpanded = overflowing && expanded

  useLayoutEffect(() => {
    const list = listRef.current
    if (!list) return
    const measure = () => {
      // Sum the unwrapped row width even when the visible list wraps to two rows.
      const gap = Number.parseFloat(getComputedStyle(list).columnGap) || 0
      const buttons = Array.from(list.children)
      const width = buttons.reduce((sum, button) => sum + button.getBoundingClientRect().width, 0)
      setOverflowing(width + Math.max(0, buttons.length - 1) * gap > list.clientWidth)
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(list)
    for (const button of list.children) observer.observe(button)
    return () => observer.disconnect()
  }, [tagOptions, i18n.resolvedLanguage])

  return (
    <div
      className={cn(
        QUICK_PANEL_FOOTER_CLASS_NAME,
        'min-w-0 gap-2 bg-muted/5',
        showExpanded && 'h-18'
      )}
    >
      <div
        id={listId}
        ref={listRef}
        className={cn(
          'flex min-w-0 flex-1 gap-0.5',
          showExpanded
            ? 'max-h-13 flex-wrap content-start overflow-y-auto'
            : 'items-center overflow-x-auto'
        )}
        data-testid="quick-panel-tag-filter-list"
      >
        {tagOptions.map(tag => {
          const active = selectedTags.has(tag.id)
          const label = t(`history.type.${tag.id}`, { defaultValue: tag.id })

          return (
            <button
              key={tag.id}
              type="button"
              aria-pressed={active}
              onClick={() => {
                const nextTags = new Set(selectedTags)
                if (active) nextTags.delete(tag.id)
                else nextTags.add(tag.id)
                onChange(nextTags.size > 0 ? [...nextTags].join(',') : null)
              }}
              className={cn(
                'inline-flex h-6 shrink-0 items-center gap-1 whitespace-nowrap rounded-md px-1.5 text-[11px] transition-colors',
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
      <button
        type="button"
        aria-label={t(showExpanded ? 'clipboard.item.collapse' : 'clipboard.item.expand')}
        aria-expanded={showExpanded}
        aria-controls={listId}
        aria-hidden={!overflowing}
        disabled={!overflowing}
        className={cn(
          'mb-2 flex size-6 shrink-0 self-end items-center justify-center rounded-md hover:bg-muted hover:text-foreground',
          !overflowing && 'invisible'
        )}
        onClick={() => setExpanded(value => !value)}
      >
        {showExpanded ? <ChevronDown className="size-4" /> : <ChevronUp className="size-4" />}
      </button>
    </div>
  )
}

export default QuickPanelTagFilterBar
