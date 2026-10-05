import { Check, type LucideIcon } from 'lucide-react'
import { Fragment, useEffect, useRef } from 'react'
import { cn } from '@/lib/utils'
import type { Dimension } from './composite-search-model'
import { DIMENSION_CHIP_CLASS } from './dimension-style'

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
  /** Hit count as words ("3 items"), shown by the list variant. */
  countLabel?: string
  /** The value's dimension; the list variant tints its chip by it. Absent on
   * syntax seeds (`type:`). */
  dimension?: Dimension
  /** Nothing matches under the other active filters: the chip dims. */
  muted?: boolean
}

interface SuggestionPanelProps {
  panelId: string
  title: string
  options: PanelOption[]
  highlightIndex: number
  onSelect: (index: number) => void
  onHighlight: (index: number) => void
  /** `list`: HList.dc.html type-ahead — chip-styled values, word counts, a
   * keyboard footer. */
  variant?: 'compact' | 'list'
  footer?: string
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
  variant = 'compact',
  footer,
}: SuggestionPanelProps) {
  // Keep the keyboard-highlighted row scrolled into view as arrow keys move it
  // past the panel's visible bounds.
  const activeRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: 'nearest' })
  }, [highlightIndex])

  if (variant === 'list') {
    return (
      <div className="absolute inset-x-0 top-full z-50 mt-1.5 overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-[0_20px_48px_rgb(14_15_18/0.2)]">
        <div
          id={panelId}
          role="listbox"
          aria-label={title}
          className="max-h-80 overflow-y-auto p-1.5"
        >
          {options.map((opt, i) => {
            const Icon = opt.icon
            const active = i === highlightIndex
            return (
              <Fragment key={opt.id}>
                {opt.header && (
                  <div className="px-2.5 pb-1.5 pt-2 text-ui-caption font-semibold uppercase text-muted-foreground">
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
                    'flex h-9 w-full items-center gap-3 rounded-lg px-2.5 text-left',
                    active && 'bg-history-accent-soft'
                  )}
                >
                  {opt.dimension ? (
                    // A value: the chip it turns into, in its dimension's tint.
                    <span
                      className={cn(
                        'inline-flex h-5.5 shrink-0 items-center rounded-full px-2 text-ui-caption font-semibold',
                        opt.muted
                          ? 'bg-muted text-muted-foreground'
                          : DIMENSION_CHIP_CLASS[opt.dimension]
                      )}
                    >
                      {opt.dimension === 'tag' ? `#${opt.label}` : opt.label}
                    </span>
                  ) : (
                    <span className="inline-flex h-5.5 shrink-0 items-center gap-1.5 rounded-full bg-foreground/8 px-2 text-ui-caption font-semibold text-foreground">
                      <Icon className="size-3 opacity-70" aria-hidden="true" />
                      {opt.label}
                    </span>
                  )}
                  <span className="min-w-0 flex-1 truncate text-ui-caption text-muted-foreground">
                    {opt.countLabel ?? opt.hint}
                  </span>
                  {opt.isActive && <Check className="size-3 shrink-0 text-primary" />}
                  {active && (
                    <span className="font-mono text-ui-caption text-muted-foreground">↵</span>
                  )}
                </button>
              </Fragment>
            )
          })}
        </div>
        {footer && (
          <div className="mx-1.5 border-t border-border/60 px-2.5 pb-2 pt-2 text-ui-caption text-muted-foreground">
            {footer}
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="absolute inset-x-0 top-full z-50 mt-1 overflow-hidden rounded-2xl border border-border/40 bg-popover text-popover-foreground shadow-lg">
      <div id={panelId} role="listbox" aria-label={title} className="max-h-72 overflow-y-auto p-1">
        {options.map((opt, i) => {
          const Icon = opt.icon
          const active = i === highlightIndex
          return (
            <Fragment key={opt.id}>
              {opt.header && (
                <div className="px-2 pb-1 pt-2 text-ui-caption font-medium uppercase text-muted-foreground/40">
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
                  'flex h-8 w-full items-center gap-2 rounded-xl px-2 text-left text-ui-body',
                  active ? 'bg-foreground/8 text-foreground' : 'text-muted-foreground'
                )}
              >
                <Icon className="size-3.5 shrink-0 opacity-70" />
                <span className="flex-1 truncate">{opt.label}</span>
                {opt.hint && (
                  <span className="font-mono text-ui-caption text-muted-foreground/40">
                    {opt.hint}
                  </span>
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
