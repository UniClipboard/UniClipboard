import { SlidersHorizontal, Search, X } from 'lucide-react'
import { Kbd } from '@/components/ui/kbd'
import { cn } from '@/lib/utils'
import type { ChipData } from './composite-search-model'
import type { Dimension } from './composite-search-model'
import { DIMENSION_INK_CLASS } from './dimension-style'
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
  /** The dimension of the filter token being typed, if any; the list
   * variant sets the token in mono, in that dimension's color. */
  typingDimension?: Dimension
  /** Keyboard help under the list variant's suggestions. */
  suggestionsFooter?: string
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
  typingDimension,
  suggestionsFooter,
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
            'flex items-center transition-colors',
            list
              ? cn(
                  // HList.dc.html query bar: a 1.5px border that darkens on
                  // focus, ringed in the accent's soft tint.
                  'min-h-11 gap-1.25 rounded-[11px] border-[1.5px] py-1.75 pl-3 pr-2 focus-within:ring-4 focus-within:ring-history-focus-ring',
                  open
                    ? 'absolute inset-x-0 top-0 z-40 flex-wrap border-foreground bg-background ring-4 ring-history-focus-ring'
                    : 'relative flex-nowrap overflow-hidden border-input bg-muted/60 focus-within:border-foreground focus-within:bg-background'
                )
              : cn(
                  'min-h-7 gap-1.5 rounded-2xl border py-1 pl-3',
                  open
                    ? 'absolute inset-x-0 top-0 z-40 flex-wrap border-border bg-popover pr-9 shadow-md'
                    : 'relative flex-nowrap overflow-hidden border-border/60 bg-muted/70 pr-3 focus-within:border-border focus-within:bg-muted'
                )
          )}
        >
          <Search
            className={cn(
              'shrink-0',
              list ? 'mr-0.75 size-3.75 text-foreground' : 'size-3.5 text-muted-foreground/50'
            )}
            strokeWidth={list ? 2.2 : undefined}
          />
          {visibleChips.map(chip => (
            <FilterChip
              key={chip.dimension}
              dimension={chip.dimension}
              variant={variant}
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
            // The list variant keeps a prompt beside its chips (HList.dc.html).
            placeholder={chips.length === 0 || list ? placeholder : ''}
            className={cn(
              'min-w-0 flex-1 bg-transparent text-ui-body outline-none placeholder:text-muted-foreground/50',
              list && typingDimension
                ? cn('font-mono', DIMENSION_INK_CLASS[typingDimension])
                : 'text-foreground'
            )}
          />
          {shortcutHint &&
            (list ? (
              // Outlined and kept while typing (HList.dc.html).
              <Kbd className="shrink-0 border border-input bg-transparent px-1.5 font-mono">
                {shortcutHint}
              </Kbd>
            ) : (
              !open && <Kbd className="shrink-0">{shortcutHint}</Kbd>
            ))}
          {!list && totalCount > 0 && !open && chips.length === 0 && (
            <span className="shrink-0 text-ui-caption tabular-nums text-muted-foreground/40">
              {countLabel}
            </span>
          )}
          {/* The list variant clears from the facet row's Clear instead. */}
          {hasContent && !list && (
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
              variant={variant}
              footer={suggestionsFooter}
            />
          )}
        </div>
      </div>
    </div>
  )
}

export default CompositeSearchInput
