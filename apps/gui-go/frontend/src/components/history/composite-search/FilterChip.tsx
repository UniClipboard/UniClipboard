import { X, type LucideIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import type { Dimension } from './composite-search-model'
import { DIMENSION_CHIP_CLASS, DIMENSION_CHIP_KEY } from './dimension-style'

interface FilterChipProps {
  dimension: Dimension
  icon: LucideIcon
  label: string
  /** `list`: HList.dc.html's 28px chip, tinted by dimension and named by its
   * syntax key ("from arch-desktop"); `compact`: the small neutral pill. */
  variant?: 'compact' | 'list'
  /** Re-open the suggestion panel for this dimension to change its value. */
  onActivate: () => void
  /** Reset this dimension to its default (removes the chip). */
  onClear: () => void
}

/**
 * A single active-filter token inside the composite search box. The body
 * re-opens the dimension's candidates; the trailing `×` clears it. Both buttons
 * suppress the default mousedown blur so the input keeps focus and the panel
 * stays open.
 */
function FilterChip({
  dimension,
  icon: Icon,
  label,
  variant = 'compact',
  onActivate,
  onClear,
}: FilterChipProps) {
  const { t } = useTranslation()
  const removeLabel = t('history.composite.removeFilter', { filter: label })

  if (variant === 'list') {
    return (
      <span
        className={cn(
          'inline-flex h-7 shrink-0 items-center gap-1 rounded-[7px] pl-2.25 pr-0.75 text-ui-caption',
          DIMENSION_CHIP_CLASS[dimension]
        )}
      >
        <button
          type="button"
          onMouseDown={e => e.preventDefault()}
          onClick={onActivate}
          className="inline-flex items-center gap-1.25 whitespace-nowrap outline-none"
        >
          <span className="opacity-70">{DIMENSION_CHIP_KEY[dimension]}</span>
          <span className="max-w-[10rem] truncate font-semibold">{label}</span>
        </button>
        <button
          type="button"
          onMouseDown={e => e.preventDefault()}
          onClick={onClear}
          aria-label={removeLabel}
          className="inline-flex size-5 items-center justify-center rounded-[5px] opacity-80 hover:bg-current/10 hover:opacity-100"
        >
          <X className="size-2.5" strokeWidth={3} />
        </button>
      </span>
    )
  }

  return (
    <span className="inline-flex h-5 items-center gap-1 rounded-full bg-foreground/8 pl-2 pr-0.5 text-ui-caption font-medium text-foreground">
      <button
        type="button"
        onMouseDown={e => e.preventDefault()}
        onClick={onActivate}
        className="inline-flex items-center gap-1 outline-none"
      >
        <Icon className="size-3 opacity-70" />
        <span className="max-w-[10rem] truncate">{label}</span>
      </button>
      <button
        type="button"
        onMouseDown={e => e.preventDefault()}
        onClick={onClear}
        aria-label={removeLabel}
        className="inline-flex size-4 items-center justify-center rounded-full text-muted-foreground/60 hover:bg-foreground/10 hover:text-foreground"
      >
        <X className="size-2.5" />
      </button>
    </span>
  )
}

export default FilterChip
