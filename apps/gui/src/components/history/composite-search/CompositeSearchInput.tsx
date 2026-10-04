import { SlidersHorizontal, Search, X } from 'lucide-react'
import { Kbd } from '@/components/ui/kbd'
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
  openFiltersLabel: string
  showFilterPanelButton: boolean
  onInputChange: (e: React.ChangeEvent<HTMLInputElement>) => void
  onInputKeyDown: (e: React.KeyboardEvent<HTMLInputElement>) => void
  openOnFocus: boolean
  onOpenChange: (open: boolean) => void
  onOpenFilters: () => void
  onClearAll: () => void
  onSeedDimension: (dimension: Dimension) => void
  onResetDimension: (dimension: Dimension) => void
  onSelectOption: (index: number) => void
  onHighlight: (index: number) => void
  /** `list`: the History list column's full-size field (HList.dc.html query
   * bar). The result count then lives in the page's summary row instead. */
  variant?: 'compact' | 'list'
  /** Keyboard shortcut shown at the field's end while closed, e.g. ⌘F. */
  shortcutHint?: string
  className?: string
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
  openFiltersLabel,
  showFilterPanelButton,
  onInputChange,
  onInputKeyDown,
  openOnFocus,
  onOpenChange,
  onOpenFilters,
  onClearAll,
  onSeedDimension,
  onResetDimension,
  onSelectOption,
  onHighlight,
  variant = 'compact',
  shortcutHint,
  className,
}: CompositeSearchInputProps) {
  const list = variant === 'list'
  return (
    <div className={cn('flex min-h-7 w-full items-start gap-1.5', className)}>
      {showFilterPanelButton && (
        <button
          type="button"
          aria-label={openFiltersLabel}
          aria-expanded={expanded}
          aria-controls={expanded ? panelId : undefined}
          onMouseDown={e => e.preventDefault()}
          onClick={onOpenFilters}
          className={cn(
            'inline-flex size-7 shrink-0 items-center justify-center rounded-2xl border transition-colors',
            expanded
              ? 'border-border bg-popover text-foreground shadow-sm'
              : 'border-border/60 bg-muted/70 text-muted-foreground/60 hover:bg-muted hover:text-foreground'
          )}
        >
          <SlidersHorizontal className="size-3.5" />
        </button>
      )}

      <div className={cn('relative min-w-0 flex-1', list ? 'min-h-11' : 'min-h-7')}>
        <div
          data-slot="composite-search-field"
          data-state={open ? 'open' : 'closed'}
          className={cn(
            'flex items-center gap-1.5 border transition-colors',
            list ? 'min-h-11 rounded-xl py-1.5 pl-3' : 'min-h-7 rounded-2xl py-1 pl-3',
            open
              ? cn(
                  'absolute inset-x-0 top-0 z-40 flex-wrap bg-popover pr-9 shadow-md',
                  list ? 'border-foreground' : 'border-border'
                )
              : cn(
                  'relative flex-nowrap overflow-hidden pr-3',
                  list
                    ? 'border-input bg-muted/60 focus-within:border-foreground focus-within:bg-background'
                    : 'border-border/60 bg-muted/70 focus-within:border-border focus-within:bg-muted'
                )
          )}
        >
          <Search
            className={cn(
              'shrink-0',
              list ? 'size-4 text-foreground' : 'size-3.5 text-muted-foreground/50'
            )}
          />
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
              className="inline-flex h-5 shrink-0 items-center rounded-full bg-foreground/8 px-2 text-ui-body font-medium text-muted-foreground hover:text-foreground"
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
            autoComplete="off"
            spellCheck={false}
            value={buffer}
            onChange={onInputChange}
            onKeyDown={onInputKeyDown}
            onFocus={() => {
              if (openOnFocus) onOpenChange(true)
            }}
            onBlur={() => onOpenChange(false)}
            placeholder={chips.length === 0 ? placeholder : ''}
            className="min-w-0 flex-1 bg-transparent text-ui-body text-foreground outline-none placeholder:text-muted-foreground/50"
          />
          {shortcutHint && !open && <Kbd className="shrink-0">{shortcutHint}</Kbd>}
          {!list && totalCount > 0 && !open && chips.length === 0 && (
            <span className="shrink-0 text-ui-caption tabular-nums text-muted-foreground/40">
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
    </div>
  )
}

export default CompositeSearchInput
