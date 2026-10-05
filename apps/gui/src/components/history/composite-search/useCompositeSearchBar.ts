import { useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import { readHistorySessionSnapshot } from '@/hooks/historySessionSnapshot'
import { splitSearchTags, type SearchTagOption } from '@/lib/search-tags'
import {
  applyDimensionValue,
  buildAllCandidates,
  buildCandidateCountQueries,
  buildCandidateTotalQueries,
  buildCandidates,
  buildChips,
  buildSyntaxSuggestions,
  buildTokenText,
  DIMENSION_LABEL_KEYS,
  parseBuffer,
  resetDimensionValue,
  SYNTAX_KEYS,
  type CandidateItem,
  type Dimension,
  type DimensionHandlers,
  type SourceOption,
} from './composite-search-model'
import { DIMENSION_CHIP_KEY } from './dimension-style'
import type { PanelOption } from './SuggestionPanel'
import { type FetchSearchCounts, useSearchCounts } from './useSearchCounts'

export interface CompositeSearchBarProps {
  contentFilter: Filter
  sourceFilter: string | null
  tagFilter: string | null
  timeRange: TimeRangePreset
  onContentFilterChange: (filter: Filter) => void
  onTagFilterChange: (tag: string | null) => void
  onSourceFilterChange: (id: string | null) => void
  onTimeRangeChange: (preset: TimeRangePreset) => void
  extensionFilter: string | null
  onExtensionFilterChange: (extension: string | null) => void
  onQueryChange: (text: string) => void
  onQuerySubmit: (text: string) => void
  sourceOptions: SourceOption[]
  tagOptions: SearchTagOption[]
  totalCount: number
  inputRef: React.RefObject<HTMLInputElement | null>
  onUnhandledKeyDown?: (e: React.KeyboardEvent<HTMLInputElement>) => void
  clearShortcutEnabled?: boolean
  suggestionActivation?: 'focus' | 'intentional'
  showFilterPanelButton?: boolean
  /** Field size; see `CompositeSearchInput`. */
  variant?: 'compact' | 'list'
  shortcutHint?: string
  /** Enables per-candidate hit counts; omitted → no counts, no network. */
  fetchCounts?: FetchSearchCounts
  className?: string
}

export function useCompositeSearchBar({
  contentFilter,
  sourceFilter,
  tagFilter,
  timeRange,
  onContentFilterChange,
  onTagFilterChange,
  onSourceFilterChange,
  onTimeRangeChange,
  extensionFilter,
  onExtensionFilterChange,
  onQueryChange,
  onQuerySubmit,
  sourceOptions,
  tagOptions,
  inputRef,
  onUnhandledKeyDown,
  suggestionActivation = 'focus',
  fetchCounts,
  variant = 'compact',
}: CompositeSearchBarProps) {
  const { t } = useTranslation()
  // Seed the text buffer from the restored session query so the box reflects an
  // active text filter after navigation/session restore (the data layer restores
  // `searchQuery`, but the buffer is otherwise local and would start empty —
  // leaving the list filtered while the box looks blank and Escape/clear inert).
  const [buffer, setBuffer] = useState(
    () => readHistorySessionSnapshot()?.searchState.searchQuery ?? ''
  )
  const [open, setOpen] = useState(false)
  const [highlight, setHighlight] = useState(-1)
  const panelId = useId()
  const current = {
    type: contentFilter,
    tag: tagFilter,
    source: sourceFilter,
    time: timeRange,
    extension: extensionFilter,
  }
  const chips = buildChips({ t, sourceOptions, tagOptions, current })
  const parsed = parseBuffer(buffer)
  const inToken = parsed.kind === 'token'
  const tokenDimension = parsed.kind === 'token' ? parsed.dimension : undefined
  // The list variant's typed token suggests values *starting with* it.
  const listToken = variant === 'list' && parsed.kind === 'token'
  const candidates: CandidateItem[] = inToken
    ? buildCandidates(parsed.dimension, parsed.partial, {
        t,
        sourceOptions,
        tagOptions,
        current,
        prefixOnly: variant === 'list',
      })
    : buildAllCandidates(buffer, { t, sourceOptions, tagOptions, current })
  const syntaxSuggestions =
    inToken || buffer.trimStart().startsWith('#') ? [] : buildSyntaxSuggestions(buffer, t)
  const expanded = open && syntaxSuggestions.length + candidates.length > 0
  // Only a single typed dimension stays within one count batch; the flat
  // all-dimension panel would not.
  const candidateCounts = useSearchCounts(
    expanded && inToken ? buildCandidateCountQueries(candidates, current) : null,
    fetchCounts
  )
  // List variant (HList.dc.html B1): the other active filters qualify the
  // typed dimension — the header reads "Tags starting with "re" · from
  // arch-desktop" and each value counts "12 items · 4 from arch-desktop".
  const contextChips = listToken ? chips.filter(chip => chip.dimension !== tokenDimension) : []
  const context = contextChips
    .map(chip => `${DIMENSION_CHIP_KEY[chip.dimension]} ${chip.label}`)
    .join(' · ')
  const candidateTotals = useSearchCounts(
    expanded && contextChips.length > 0 ? buildCandidateTotalQueries(candidates) : null,
    fetchCounts
  )
  const listHeader =
    parsed.kind === 'token'
      ? [
          t(
            parsed.partial
              ? 'history.composite.header.startingWith'
              : 'history.composite.header.all',
            {
              dimension: t(`history.composite.header.dimension.${parsed.dimension}`),
              partial: parsed.partial,
            }
          ),
          context,
        ]
          .filter(Boolean)
          .join(' · ')
      : undefined
  const listHint = (i: number): { countLabel?: string; muted?: boolean } => {
    const inFilters = candidateCounts?.[i]
    if (inFilters === undefined) return {}
    if (!context) return { countLabel: t('history.subtitle', { count: inFilters }) }
    const total = candidateTotals?.[i]
    const qualified =
      inFilters > 0
        ? t('history.composite.inContext', { count: inFilters, context })
        : t('history.composite.noneInContext', { context })
    return {
      countLabel:
        total === undefined
          ? qualified
          : `${t('history.subtitle', { count: total })} · ${qualified}`,
      muted: inFilters === 0,
    }
  }
  const options: PanelOption[] = [
    ...syntaxSuggestions.map(s => ({
      id: `seed-${s.dimension}`,
      label: s.label,
      icon: s.icon,
      hint: s.hint,
    })),
    ...candidates.map((c, i) => ({
      id: c.id,
      dimension: c.dimension,
      label: c.label,
      icon: c.icon,
      isActive: c.isActive,
      // The list-column field also names the dimension being typed (HList B1).
      header: listToken
        ? i === 0
          ? listHeader
          : undefined
        : !inToken && (i === 0 || candidates[i - 1].dimension !== c.dimension)
          ? t(DIMENSION_LABEL_KEYS[c.dimension])
          : undefined,
      hint: candidateCounts?.[i]?.toLocaleString(),
      ...(variant === 'list'
        ? listHint(i)
        : {
            countLabel:
              candidateCounts?.[i] === undefined
                ? undefined
                : t('history.subtitle', { count: candidateCounts[i] }),
          }),
    })),
  ]
  const clampedHighlight =
    highlight < 0 || options.length === 0 ? -1 : Math.min(highlight, options.length - 1)
  const suggestionsHandleKeys = expanded || suggestionActivation === 'focus'
  const handlers: DimensionHandlers = {
    onContentFilterChange,
    onTagFilterChange,
    onSourceFilterChange,
    onTimeRangeChange,
    onExtensionFilterChange,
  }
  const resetDimension = (dimension: Dimension) => resetDimensionValue(dimension, handlers)

  const applyCandidate = (c: CandidateItem) => {
    applyDimensionValue(c.dimension, c.value, handlers, current)
    resetBuffer()
  }

  const resetBuffer = () => {
    setBuffer('')
    onQueryChange('')
    setHighlight(-1)
    setOpen(suggestionActivation === 'focus')
    inputRef.current?.focus()
  }

  const seedDimension = (dimension: Dimension) => {
    // The tag dimension's syntax key (`#`) is the whole prefix; the others take a
    // trailing colon (`type:`). Seeding `#:` would make `parseBuffer` treat `:`
    // as the partial tag text and surface no useful matches.
    setBuffer(dimension === 'tag' ? SYNTAX_KEYS.tag : `${SYNTAX_KEYS[dimension]}:`)
    setHighlight(0)
    onQueryChange('')
    inputRef.current?.focus()
    setOpen(true)
  }

  const tryCommit = (dimension: Dimension, partial: string) => {
    const cands = buildCandidates(dimension, partial, { t, sourceOptions, tagOptions, current })
    const exact =
      cands.find(c => c.value.toLowerCase() === partial.toLowerCase()) ??
      (cands.length === 1 ? cands[0] : undefined)
    // Typing an already-selected tag keeps it; only clicking its checked row
    // toggles it off.
    if (exact?.dimension === 'tag' && exact.isActive) resetBuffer()
    else if (exact) applyCandidate(exact)
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const next = e.target.value
    const p = parseBuffer(next)
    setBuffer(next)
    setHighlight(p.kind === 'token' ? 0 : -1)
    setOpen(suggestionActivation === 'focus' || p.kind === 'token')
    if (p.kind === 'query') {
      onQueryChange(next)
    } else {
      onQueryChange('')
      if (p.committed) tryCommit(p.dimension, p.partial)
    }
  }

  const selectOption = (index: number) => {
    if (index < syntaxSuggestions.length) {
      seedDimension(syntaxSuggestions[index].dimension)
      return
    }
    const c = candidates[index - syntaxSuggestions.length]
    if (c) applyCandidate(c)
  }

  const hasContent = chips.length > 0 || buffer.length > 0
  const clearAll = ({ refocus = true }: { refocus?: boolean } = {}) => {
    resetDimension('type')
    resetDimension('tag')
    resetDimension('source')
    resetDimension('time')
    resetDimension('extension')
    setBuffer('')
    onQueryChange('')
    setHighlight(-1)
    if (refocus) inputRef.current?.focus()
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      if (open) setOpen(false)
      else if (hasContent) clearAll()
      else inputRef.current?.blur()
      return
    }
    if (e.key === 'Backspace' && buffer === '' && chips.length > 0) {
      e.preventDefault()
      const lastChip = chips[chips.length - 1]
      // The tag chip holds the whole selection: pop only its last tag back
      // into the buffer and keep the rest selected.
      const tags = splitSearchTags(current.tag)
      const value =
        lastChip.dimension === 'tag' ? tags[tags.length - 1] : String(current[lastChip.dimension])
      if (lastChip.dimension === 'tag') applyDimensionValue('tag', value, handlers, current)
      else resetDimension(lastChip.dimension)
      // Source ids are internal (`mobile_sync:did_…`); candidates also match by
      // name, so reopen with the name the user recognises.
      const editable =
        lastChip.dimension === 'source'
          ? (sourceOptions.find(o => o.id === value)?.name ?? value)
          : value
      setBuffer(buildTokenText(lastChip.dimension, editable))
      setHighlight(0)
      setOpen(true)
      return
    }
    if (
      suggestionsHandleKeys &&
      options.length > 0 &&
      (e.key === 'ArrowDown' || e.key === 'ArrowUp')
    ) {
      e.preventDefault()
      setOpen(true)
      const delta = e.key === 'ArrowDown' ? 1 : -1
      setHighlight(h =>
        h < 0 ? (delta > 0 ? 0 : options.length - 1) : (h + delta + options.length) % options.length
      )
      return
    }
    if (e.key === 'Tab' && !e.shiftKey && expanded && inToken) {
      e.preventDefault()
      selectOption(clampedHighlight >= 0 ? clampedHighlight : 0)
      return
    }
    if (e.key === 'Enter') {
      if (suggestionActivation === 'intentional' && !expanded) {
        onUnhandledKeyDown?.(e)
        return
      }
      e.preventDefault()
      if (clampedHighlight >= 0) {
        selectOption(clampedHighlight)
      } else if (inToken) {
        if (candidates.length > 0) applyCandidate(candidates[0])
      } else {
        onQuerySubmit(buffer)
        setOpen(false)
      }
      return
    }
    onUnhandledKeyDown?.(e)
  }

  return {
    t,
    buffer,
    open,
    setOpen,
    panelId,
    current,
    inToken,
    tokenDimension,
    chips,
    options,
    visibleChips: open ? chips : chips.slice(0, 2),
    hiddenChipCount: open ? 0 : Math.max(chips.length - 2, 0),
    clampedHighlight,
    expanded,
    hasContent,
    handleInputChange,
    handleKeyDown,
    clearAll,
    openOnFocus: suggestionActivation === 'focus',
    openFilters: () => {
      // Only surface the suggestion panel; keep any active text query intact so
      // reaching for a filter doesn't wipe what the user already typed.
      setHighlight(-1)
      setOpen(true)
      inputRef.current?.focus()
    },
    seedDimension,
    resetDimension,
    selectOption,
    setHighlight,
  }
}
