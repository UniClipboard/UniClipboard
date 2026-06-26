import { Search, X } from 'lucide-react'
import { useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import { useShortcut } from '@/hooks/useShortcut'
import { cn } from '@/lib/utils'
import {
  applyDimensionValue,
  buildAllCandidates,
  buildCandidates,
  buildChips,
  buildSyntaxSuggestions,
  DIMENSION_LABEL_KEYS,
  parseBuffer,
  resetDimensionValue,
  SYNTAX_KEYS,
  type CandidateItem,
  type Dimension,
  type SourceOption,
} from './composite-search-model'
import FilterChip from './FilterChip'
import SuggestionPanel, { type PanelOption } from './SuggestionPanel'

interface CompositeSearchBarProps {
  /** Current content-type selection (maps to `activeFilter`). */
  contentFilter: Filter
  /** Current source-device id, or null for all (maps to `sourceFilter`). */
  sourceFilter: string | null
  /** Current time preset (maps to `timeRange`). */
  timeRange: TimeRangePreset
  onContentFilterChange: (filter: Filter) => void
  onSourceFilterChange: (id: string | null) => void
  onTimeRangeChange: (preset: TimeRangePreset) => void
  /** Free-text query (debounced into a search by the parent). */
  onQueryChange: (text: string) => void
  /** Submit the given query text immediately (Enter). */
  onQuerySubmit: (text: string) => void
  sourceOptions: SourceOption[]
  /** Browse-list size, shown as a muted count. */
  totalCount: number
  /** Shared with the parent so CMD/Ctrl+F can focus the input. */
  inputRef: React.RefObject<HTMLInputElement | null>
}

/**
 * Composite search box: one input that hosts free-text search, removable filter
 * chips (content type / source / time), and a keyboard-or-mouse suggestion
 * panel. It is a pure skin over the History page's four state values — every
 * selection is dispatched back through the `on*Change` props; no search data
 * flow lives here.
 */
function CompositeSearchBar({
  contentFilter,
  sourceFilter,
  timeRange,
  onContentFilterChange,
  onSourceFilterChange,
  onTimeRangeChange,
  onQueryChange,
  onQuerySubmit,
  sourceOptions,
  totalCount,
  inputRef,
}: CompositeSearchBarProps) {
  const { t } = useTranslation()
  const [buffer, setBuffer] = useState('')
  const [open, setOpen] = useState(false)
  // -1 means "no active highlight": Enter then submits the text query instead of
  // applying a value. Arrow keys move it onto the list.
  const [highlight, setHighlight] = useState(-1)
  const panelId = useId()

  const current = { type: contentFilter, source: sourceFilter, time: timeRange }
  const chips = buildChips({ t, sourceOptions, current })
  const parsed = parseBuffer(buffer)

  // Token mode (`type:` …) narrows to one dimension; otherwise the panel shows
  // every filter value across all dimensions, narrowed by the typed text. Either
  // way the list is a flat array of directly-selectable values — no two-step
  // "pick a category first" detour.
  const inToken = parsed.kind === 'token'
  const candidates: CandidateItem[] = inToken
    ? buildCandidates(parsed.dimension, parsed.partial, { t, sourceOptions, current })
    : buildAllCandidates(buffer, { t, sourceOptions, current })

  // Syntax-prefix hints (e.g. typing `t` suggests `type:`) lead the list in
  // free-text mode so the keyboard token syntax stays discoverable.
  const syntaxSuggestions = inToken ? [] : buildSyntaxSuggestions(buffer, t)

  const options: PanelOption[] = [
    ...syntaxSuggestions.map(s => ({
      id: `seed-${s.dimension}`,
      label: s.label,
      icon: s.icon,
      hint: s.hint,
    })),
    ...candidates.map((c, i) => ({
      id: c.id,
      label: c.label,
      icon: c.icon,
      isActive: c.isActive,
      // Group header above the first value of each dimension (skip in token
      // mode, where every row shares one dimension).
      header:
        !inToken && (i === 0 || candidates[i - 1].dimension !== c.dimension)
          ? t(DIMENSION_LABEL_KEYS[c.dimension])
          : undefined,
    })),
  ]

  const clampedHighlight =
    highlight < 0 || options.length === 0 ? -1 : Math.min(highlight, options.length - 1)
  // The suggestion panel is only mounted when there are options to show; the
  // combobox ARIA attributes mirror that so AT only sees the popup when it
  // actually exists.
  const expanded = open && options.length > 0

  const handlers = { onContentFilterChange, onSourceFilterChange, onTimeRangeChange }
  const resetDimension = (dimension: Dimension) => resetDimensionValue(dimension, handlers)

  const applyCandidate = (c: CandidateItem) => {
    applyDimensionValue(c.dimension, c.value, handlers)
    setBuffer('')
    onQueryChange('')
    setHighlight(-1)
    // Close so the freshly-filtered results are visible; focus stays in the
    // input, and arrowing or typing re-opens the panel to add more filters.
    setOpen(false)
    inputRef.current?.focus()
  }

  // Chip click re-opens its dimension's values by seeding `type:` etc.
  const seedDimension = (dimension: Dimension) => {
    setBuffer(`${SYNTAX_KEYS[dimension]}:`)
    setHighlight(-1)
    onQueryChange('')
    inputRef.current?.focus()
    setOpen(true)
  }

  // Commit a fully-typed token like `type:image ` into its chip.
  const tryCommit = (dimension: Dimension, partial: string) => {
    const cands = buildCandidates(dimension, partial, { t, sourceOptions, current })
    const exact =
      cands.find(c => c.value.toLowerCase() === partial.toLowerCase()) ??
      (cands.length === 1 ? cands[0] : undefined)
    if (exact) applyCandidate(exact)
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const next = e.target.value
    setBuffer(next)
    setHighlight(-1)
    setOpen(true)
    const p = parseBuffer(next)
    if (p.kind === 'query') {
      onQueryChange(next)
    } else {
      // While building a token, keep the query empty so it can't leak in.
      onQueryChange('')
      if (p.committed) tryCommit(p.dimension, p.partial)
    }
  }

  const selectOption = (index: number) => {
    // Leading rows are syntax-prefix hints (seed the token); the rest are values.
    if (index < syntaxSuggestions.length) {
      seedDimension(syntaxSuggestions[index].dimension)
      return
    }
    const c = candidates[index - syntaxSuggestions.length]
    if (c) applyCandidate(c)
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      // Step down one level per press: close the panel first, then exit search
      // entirely (drop every chip and the typed text), and finally blur.
      if (open) setOpen(false)
      else if (hasContent) clearAll()
      else inputRef.current?.blur()
      return
    }
    if (e.key === 'Backspace' && buffer === '' && chips.length > 0) {
      e.preventDefault()
      resetDimension(chips[chips.length - 1].dimension)
      return
    }
    if (options.length > 0 && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      e.preventDefault()
      setOpen(true)
      const delta = e.key === 'ArrowDown' ? 1 : -1
      setHighlight(h =>
        h < 0 ? (delta > 0 ? 0 : options.length - 1) : (h + delta + options.length) % options.length
      )
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      if (clampedHighlight >= 0) {
        // A highlighted value (arrowed-to or hovered) wins.
        selectOption(clampedHighlight)
      } else if (inToken) {
        // `type:image` + Enter applies the first/only match without arrowing.
        if (candidates.length > 0) applyCandidate(candidates[0])
      } else {
        // Free text with no highlight: run it as a full-text search.
        onQuerySubmit(buffer)
        setOpen(false)
      }
    }
  }

  const hasContent = chips.length > 0 || buffer.length > 0
  // `refocus` keeps the cursor in the box after a clear triggered from inside it
  // (in-input Esc, clear button). A page-level Esc (focus elsewhere) passes
  // refocus:false so clearing the filters never yanks focus into the input.
  const clearAll = ({ refocus = true }: { refocus?: boolean } = {}) => {
    resetDimension('type')
    resetDimension('source')
    resetDimension('time')
    setBuffer('')
    onQueryChange('')
    setHighlight(-1)
    if (refocus) inputRef.current?.focus()
  }

  // Esc while focus is OUTSIDE the input (e.g. browsing the grid) still drops
  // every filter. The in-input Esc path lives in handleKeyDown;
  // enableOnFormTags:false stops the two from both firing while typing.
  useShortcut({
    key: 'esc',
    scope: 'clipboard',
    enabled: hasContent,
    handler: () => clearAll({ refocus: false }),
    enableOnFormTags: false,
  })

  return (
    <div className="relative w-full">
      <div className="flex min-h-7 flex-wrap items-center gap-1.5 rounded-2xl bg-muted/40 px-3 py-1 transition-colors focus-within:bg-muted/60">
        <Search className="size-3.5 shrink-0 text-muted-foreground/50" />
        {chips.map(chip => (
          <FilterChip
            key={chip.dimension}
            icon={chip.icon}
            label={chip.label}
            onActivate={() => seedDimension(chip.dimension)}
            onClear={() => resetDimension(chip.dimension)}
          />
        ))}
        <input
          ref={inputRef}
          type="text"
          role="combobox"
          aria-label={t('history.composite.title')}
          aria-autocomplete="list"
          aria-expanded={expanded}
          aria-controls={expanded ? panelId : undefined}
          aria-activedescendant={
            expanded && clampedHighlight >= 0 ? options[clampedHighlight]?.id : undefined
          }
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          value={buffer}
          onChange={handleInputChange}
          onKeyDown={handleKeyDown}
          onFocus={() => setOpen(true)}
          onBlur={() => setOpen(false)}
          placeholder={chips.length === 0 ? t('history.searchPlaceholder') : ''}
          className="min-w-[6rem] flex-1 bg-transparent text-[12px] text-foreground outline-none placeholder:text-muted-foreground/50"
        />
        {totalCount > 0 && (
          <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground/40">
            {t('history.subtitle', { count: totalCount })}
          </span>
        )}
        {hasContent && (
          <button
            type="button"
            onMouseDown={e => e.preventDefault()}
            onClick={() => clearAll()}
            aria-label={t('history.composite.clearAll')}
            className={cn(
              'inline-flex size-5 shrink-0 items-center justify-center rounded-full',
              'text-muted-foreground/60 hover:bg-foreground/10 hover:text-foreground'
            )}
          >
            <X className="size-3" />
          </button>
        )}
      </div>
      {expanded && (
        <SuggestionPanel
          panelId={panelId}
          title={t('history.composite.title')}
          options={options}
          highlightIndex={clampedHighlight}
          onSelect={selectOption}
          onHighlight={setHighlight}
        />
      )}
    </div>
  )
}

export default CompositeSearchBar
