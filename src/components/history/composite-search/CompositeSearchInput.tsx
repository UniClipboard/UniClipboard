import { Search, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { ChipData } from './composite-search-model'
import type { Dimension } from './composite-search-model'
import FilterChip from './FilterChip'
import SuggestionPanel, { type PanelOption } from './SuggestionPanel'

interface CompositeSearchInputProps {
  inputRef: React.RefObject<HTMLInputElement | null>
  buffer: string
  open: boolean
  panelId: string
  chips: ChipData[]
  visibleChips: ChipData[]
  hiddenChipCount: number
  options: PanelOption[]
  expanded: boolean
  clampedHighlight: number
  hasContent: boolean
  totalCount: number
  title: string
  placeholder: string
  countLabel: string
  moreFiltersLabel: string
  clearAllLabel: string
  onInputChange: (e: React.ChangeEvent<HTMLInputElement>) => void
  onInputKeyDown: (e: React.KeyboardEvent<HTMLInputElement>) => void
  onOpenChange: (open: boolean) => void
  onClearAll: () => void
  onSeedDimension: (dimension: Dimension) => void
  onResetDimension: (dimension: Dimension) => void
  onSelectOption: (index: number) => void
  onHighlight: (index: number) => void
}

function CompositeSearchInput({
  inputRef,
  buffer,
  open,
  panelId,
  chips,
  visibleChips,
  hiddenChipCount,
  options,
  expanded,
  clampedHighlight,
  hasContent,
  totalCount,
  title,
  placeholder,
  countLabel,
  moreFiltersLabel,
  clearAllLabel,
  onInputChange,
  onInputKeyDown,
  onOpenChange,
  onClearAll,
  onSeedDimension,
  onResetDimension,
  onSelectOption,
  onHighlight,
}: CompositeSearchInputProps) {
  return (
    <div className="relative min-h-7 w-full">
      <div
        className={cn(
          'flex min-h-7 items-center gap-1.5 rounded-2xl border py-1 pl-3 transition-colors',
          open
            ? 'absolute inset-x-0 top-0 z-40 flex-wrap border-border bg-popover pr-9 shadow-md'
            : 'relative flex-nowrap overflow-hidden border-border/60 bg-muted/70 pr-3 focus-within:border-border focus-within:bg-muted'
        )}
      >
        <Search className="size-3.5 shrink-0 text-muted-foreground/50" />
        {visibleChips.map(chip => (
          <FilterChip
            key={chip.dimension}
            icon={chip.icon}
            label={chip.label}
            onActivate={() => onSeedDimension(chip.dimension)}
            onClear={() => onResetDimension(chip.dimension)}
          />
        ))}
        {!open && hiddenChipCount > 0 && (
          <button
            type="button"
            onMouseDown={e => e.preventDefault()}
            onClick={() => {
              onOpenChange(true)
              inputRef.current?.focus()
            }}
            aria-label={moreFiltersLabel}
            className="inline-flex h-5 shrink-0 items-center rounded-full bg-foreground/8 px-2 text-[11px] font-medium text-muted-foreground hover:text-foreground"
          >
            +{hiddenChipCount}
          </button>
        )}
        <input
          ref={inputRef}
          type="text"
          role="combobox"
          aria-label={title}
          aria-expanded={expanded}
          aria-autocomplete="list"
          aria-controls={expanded ? panelId : undefined}
          aria-activedescendant={
            expanded && clampedHighlight >= 0 ? options[clampedHighlight]?.id : undefined
          }
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          value={buffer}
          onChange={onInputChange}
          onKeyDown={onInputKeyDown}
          onFocus={() => onOpenChange(true)}
          onBlur={() => onOpenChange(false)}
          placeholder={chips.length === 0 ? placeholder : ''}
          className="min-w-0 flex-1 bg-transparent text-[12px] text-foreground outline-none placeholder:text-muted-foreground/50"
        />
        {totalCount > 0 && !open && chips.length === 0 && (
          <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground/40">
            {countLabel}
          </span>
        )}
        {hasContent && (
          <button
            type="button"
            onMouseDown={e => e.preventDefault()}
            onClick={onClearAll}
            aria-label={clearAllLabel}
            className={cn(
              'inline-flex size-5 shrink-0 items-center justify-center rounded-full',
              'text-muted-foreground/60 hover:bg-foreground/10 hover:text-foreground',
              open && 'absolute right-2.5 top-1.5'
            )}
          >
            <X className="size-3" />
          </button>
        )}
        {expanded && (
          <SuggestionPanel
            panelId={panelId}
            title={title}
            options={options}
            highlightIndex={clampedHighlight}
            onSelect={onSelectOption}
            onHighlight={onHighlight}
          />
        )}
      </div>
    </div>
  )
}

export default CompositeSearchInput
