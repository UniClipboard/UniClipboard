import { X, type LucideIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'

interface FilterChipProps {
  icon: LucideIcon
  label: string
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
function FilterChip({ icon: Icon, label, onActivate, onClear }: FilterChipProps) {
  const { t } = useTranslation()
  return (
    <span className="inline-flex h-5 items-center gap-1 rounded-full bg-foreground/8 pl-2 pr-0.5 text-[11px] font-medium text-foreground">
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
        aria-label={t('history.composite.removeFilter', { filter: label })}
        className="inline-flex size-4 items-center justify-center rounded-full text-muted-foreground/60 hover:bg-foreground/10 hover:text-foreground"
      >
        <X className="size-2.5" />
      </button>
    </span>
  )
}

export default FilterChip
