import { Check, type LucideIcon } from 'lucide-react'
import { Fragment, useEffect, useRef } from 'react'
import { cn } from '@/lib/utils'

export interface PanelOption {
  /** Stable id, also used as the React key. */
  id: string
  label: string
  icon: LucideIcon
  /** Marks the dimension's current selection. */
  isActive?: boolean
  /** Group header rendered above this row (first value of each dimension). */
  header?: string
  /** Trailing muted hint, e.g. the syntax seed `type:`. */
  hint?: string
}

interface SuggestionPanelProps {
  panelId: string
  title: string
  options: PanelOption[]
  highlightIndex: number
  onSelect: (index: number) => void
  onHighlight: (index: number) => void
}

/**
 * Absolutely-positioned suggestion list under the composite input. Rows are
 * native buttons (focusable, keyboard-operable) kept out of the tab order
 * (`tabIndex={-1}`) so focus stays in the input; `mousedown` is suppressed so a
 * click doesn't blur the input and close the panel before it lands. The visible
 * highlight is driven by the input's arrow-key navigation, not DOM focus.
 */
function SuggestionPanel({
  panelId,
  title,
  options,
  highlightIndex,
  onSelect,
  onHighlight,
}: SuggestionPanelProps) {
  // Keep the keyboard-highlighted row scrolled into view as arrow keys move it
  // past the panel's visible bounds.
  const activeRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: 'nearest' })
  }, [highlightIndex])

  return (
    <div className="absolute inset-x-0 top-full z-50 mt-1 overflow-hidden rounded-2xl border border-border/40 bg-popover text-popover-foreground shadow-lg">
      <div id={panelId} role="listbox" aria-label={title} className="max-h-72 overflow-y-auto p-1">
        {options.map((opt, i) => {
          const Icon = opt.icon
          const active = i === highlightIndex
          return (
            <Fragment key={opt.id}>
              {opt.header && (
                <div className="px-2 pb-1 pt-2 text-[10px] font-medium uppercase tracking-wide text-muted-foreground/40">
                  {opt.header}
                </div>
              )}
              <button
                id={opt.id}
                ref={active ? activeRef : undefined}
                type="button"
                role="option"
                aria-selected={active}
                tabIndex={-1}
                onMouseDown={e => e.preventDefault()}
                onClick={() => onSelect(i)}
                onMouseEnter={() => onHighlight(i)}
                className={cn(
                  'flex h-8 w-full items-center gap-2 rounded-xl px-2 text-left text-[12px]',
                  active ? 'bg-foreground/8 text-foreground' : 'text-muted-foreground'
                )}
              >
                <Icon className="size-3.5 shrink-0 opacity-70" />
                <span className="flex-1 truncate">{opt.label}</span>
                {opt.hint && (
                  <span className="font-mono text-[11px] text-muted-foreground/40">{opt.hint}</span>
                )}
                {opt.isActive && <Check className="size-3 shrink-0 text-primary" />}
              </button>
            </Fragment>
          )
        })}
      </div>
    </div>
  )
}

export default SuggestionPanel
