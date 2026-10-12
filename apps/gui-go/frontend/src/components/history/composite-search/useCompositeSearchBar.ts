import { Plus } from 'lucide-react'
import { useId, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import {
  replaceSearchRange,
  resolveSearchInput,
  searchTokenRanges,
  type SearchSelection,
} from '@/components/history/composite-search/search-token-ranges'
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

/** The parts of a key press the search model reads; a DOM keyboard event satisfies it. */
export type SearchKeyEvent = Pick<
  React.KeyboardEvent<HTMLInputElement>,
  'key' | 'shiftKey' | 'preventDefault'
>

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
  /** Keys the model leaves alone, for a caller that handles the DOM event itself (quick panel). */
  onUnhandledKeyDown?: (e: React.KeyboardEvent<HTMLInputElement>) => void
  clearShortcutEnabled?: boolean
  suggestionActivation?: 'focus' | 'intentional'
  showFilterPanelButton?: boolean
  /** Field size; see `CompositeSearchInput`. */
  variant?: 'compact' | 'list'
  shortcutHint?: string
  /** Enables per-candidate hit counts; omitted → no counts, no network. */
  fetchCounts?: FetchSearchCounts
  /** List variant: offer "+ Create #name" after a typed `tag:` value no tag
   * has. Resolves to the new tag's id, or `null` when it was not created. */
  onCreateTag?: (name: string) => Promise<string | null>
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
  onCreateTag,
  variant = 'compact',
}: CompositeSearchBarProps) {
  const { t } = useTranslation()
  // Seed the text buffer from the restored session query so the box reflects an
  // active text filter after navigation/session restore (the data layer restores
  // `searchQuery`, but the buffer is otherwise local and would start empty —
  // leaving the list filtered while the box looks blank and Escape/clear inert).
  const [input, setInputState] = useState(() => {
    const buffer = readHistorySessionSnapshot()?.searchState.searchQuery ?? ''
    return { buffer, selection: { start: buffer.length, end: buffer.length } }
  })
  const { buffer, selection } = input
  const restoreSelection = useRef(false)
  const editRevision = useRef(0)
  const setInput = (next: typeof input) => {
    editRevision.current++
    setInputState(next)
  }
  useLayoutEffect(() => {
    if (!restoreSelection.current) return
    restoreSelection.current = false
    inputRef.current?.focus()
    inputRef.current?.setSelectionRange(selection.start, selection.end)
  }, [input, inputRef, selection])
  const setBuffer = (buffer: string) =>
    setInput({ buffer, selection: { start: buffer.length, end: buffer.length } })
  const [open, setOpenState] = useState(false)
  const setOpen = (next: boolean) => {
    if (!next) editRevision.current++
    setOpenState(next)
  }
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
  // The list variant's typed token suggests values *starting with* it.
  const tokenContext = { t, sourceOptions, tagOptions, current, prefixOnly: variant === 'list' }
  const completion = resolveSearchInput(buffer, selection, tokenContext)
  const { range: activeToken, typed, parsed, query: textQuery } = completion
  const inToken = parsed.kind === 'token'
  const tokenDimension = parsed.kind === 'token' ? parsed.dimension : undefined
  const listToken = variant === 'list' && parsed.kind === 'token'
  const candidates: CandidateItem[] = inToken
    ? completion.candidates
    : buildAllCandidates(buffer, { t, sourceOptions, tagOptions, current })
  const syntaxSuggestions = inToken ? [] : buildSyntaxSuggestions(buffer, t)
  // HList.dc.html B1: a typed tag no tag has yet ends the list with "+ Create".
  // A `#name` naming no tag stays a text search, so the
  // offer reads the active word; Enter still searches the text, and the offer
  // is picked only by arrow keys or a click.
  const createName =
    variant === 'list' && onCreateTag && typed?.kind === 'token' && typed.dimension === 'tag'
      ? typed.partial
      : ''
  const createOffer =
    createName !== '' &&
    // A builtin tag matches by id as well as its translated label.
    !buildCandidates('tag', createName, tokenContext).some(c =>
      [c.value, c.label].some(name => name.toLocaleLowerCase() === createName.toLocaleLowerCase())
    )
      ? createName
      : null
  const expanded = open && syntaxSuggestions.length + candidates.length + (createOffer ? 1 : 0) > 0
  // Only a single typed dimension stays within one count batch; the flat
  // all-dimension panel would not.
  const candidateCounts = useSearchCounts(
    expanded && inToken ? buildCandidateCountQueries(candidates, current, textQuery) : null,
    fetchCounts
  )
  // List variant (HList.dc.html B1): the other active filters qualify the
  // typed dimension — the header reads "Tags starting with "re" · from
  // arch-desktop" and each value counts "12 items · 4 from arch-desktop".
  const contextChips = listToken ? chips.filter(chip => chip.dimension !== tokenDimension) : []
  const context = [
    ...contextChips.map(chip => `${DIMENSION_CHIP_KEY[chip.dimension]} ${chip.label}`),
    ...(textQuery.trim() ? [t('history.composite.matchingText')] : []),
  ].join(' · ')
  const candidateTotals = useSearchCounts(
    expanded && inToken && context.length > 0 ? buildCandidateTotalQueries(candidates) : null,
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
    ...(createOffer
      ? [
          {
            id: 'create-tag',
            label: t('history.tags.createNamed', { name: createOffer }),
            icon: Plus,
            create: true,
            header: candidates.length === 0 ? t(DIMENSION_LABEL_KEYS.tag) : undefined,
            hint: t('history.tags.createHint'),
          },
        ]
      : []),
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
  const resetDimension = (dimension: Dimension) => {
    editRevision.current++
    resetDimensionValue(dimension, handlers)
  }

  const finishToken = () => {
    const next = activeToken ? replaceSearchRange(buffer, activeToken) : input
    restoreSelection.current = true
    setInput(next)
    onQueryChange(next.buffer.trim())
    if (next.buffer.trim()) onQuerySubmit(next.buffer.trim())
    setHighlight(-1)
    setOpen(false)
  }
  const applyCandidate = (c: CandidateItem) => {
    applyDimensionValue(c.dimension, c.value, handlers, current)
    finishToken()
  }

  const seedDimension = (dimension: Dimension) => {
    const range =
      activeToken ?? (syntaxSuggestions.length > 0 ? { start: 0, end: buffer.length } : selection)
    const prefix = range.start > 0 && !/\s/.test(buffer[range.start - 1]) ? ' ' : ''
    const suffix = range.end < buffer.length && !/\s/.test(buffer[range.end]) ? ' ' : ''
    const next = replaceSearchRange(buffer, range, `${prefix}${SYNTAX_KEYS[dimension]}${suffix}`)
    next.selection.start = next.selection.end =
      range.start + prefix.length + SYNTAX_KEYS[dimension].length
    restoreSelection.current = true
    setInput(next)
    setHighlight(0)
    onQueryChange(textQuery)
    inputRef.current?.focus()
    setOpen(true)
  }

  const changeText = (next: string, caret?: SearchSelection) => {
    let nextInput = { buffer: next, selection: caret ?? { start: next.length, end: next.length } }
    const nextCurrent = { ...current }
    const commitHandlers = {
      ...handlers,
      onTagFilterChange: (tag: string | null) => {
        nextCurrent.tag = tag
        onTagFilterChange(tag)
      },
    }
    // Closed, exact filter words can be pasted or typed together. Apply them from left to right.
    const completed = searchTokenRanges(next).filter(r => r.end < nextInput.selection.start)
    const removed: typeof completed = []
    for (const range of completed) {
      const p = range.parsed
      if (p.kind !== 'token' || !p.committed || !p.partial) continue
      const candidate = buildCandidates(p.dimension, p.partial, {
        ...tokenContext,
        current: nextCurrent,
      }).find(c =>
        [c.value, c.label].some(
          value => value.toLocaleLowerCase() === p.partial.toLocaleLowerCase()
        )
      )
      if (!candidate) continue
      if (!(candidate.dimension === 'tag' && candidate.isActive))
        applyDimensionValue(candidate.dimension, candidate.value, commitHandlers, nextCurrent)
      removed.push(range)
    }
    for (const range of removed.reverse()) {
      nextInput = {
        buffer: replaceSearchRange(nextInput.buffer, range).buffer,
        selection: {
          start: nextInput.selection.start - (range.end - range.start),
          end: nextInput.selection.end - (range.end - range.start),
        },
      }
    }
    if (!nextInput.buffer.trim()) nextInput = { buffer: '', selection: { start: 0, end: 0 } }
    if (removed.length) restoreSelection.current = true
    setInput(nextInput)
    const nextCompletion = resolveSearchInput(nextInput.buffer, nextInput.selection, tokenContext)
    const isToken = nextCompletion.parsed.kind === 'token'
    setHighlight(isToken ? 0 : -1)
    setOpen(suggestionActivation === 'focus' || isToken)
    onQueryChange(nextCompletion.query)
    if (removed.length && nextCompletion.query.trim()) onQuerySubmit(nextCompletion.query)
  }

  const changeSelection = (start: number, end: number) => {
    if (selection.start === start && selection.end === end) return
    setInput({ buffer, selection: { start, end } })
    const nextCompletion = resolveSearchInput(buffer, { start, end }, tokenContext)
    const isToken = nextCompletion.parsed.kind === 'token'
    setHighlight(isToken ? 0 : -1)
    setOpen(Boolean(nextCompletion.typed))
    onQueryChange(nextCompletion.query)
  }
  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    changeText(e.target.value, {
      start: e.target.selectionStart ?? e.target.value.length,
      end: e.target.selectionEnd ?? e.target.value.length,
    })

  const createTag = async (name: string) => {
    const revision = editRevision.current
    const tagId = await onCreateTag?.(name)
    if (!tagId || revision !== editRevision.current) return
    applyDimensionValue('tag', tagId, handlers, current)
    finishToken()
  }

  const selectOption = (index: number) => {
    if (index < syntaxSuggestions.length) {
      seedDimension(syntaxSuggestions[index].dimension)
      return
    }
    const c = candidates[index - syntaxSuggestions.length]
    if (c) applyCandidate(c)
    else if (createOffer) void createTag(createOffer)
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
    onQuerySubmit('')
    setHighlight(-1)
    if (refocus) inputRef.current?.focus()
  }

  // Only a DOM keyboard event is passed on: a value-only key press has nobody else to handle it.
  const unhandled = (e: SearchKeyEvent) => {
    if ('nativeEvent' in e) onUnhandledKeyDown?.(e as React.KeyboardEvent<HTMLInputElement>)
  }
  const handleKeyDown = (e: SearchKeyEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      if (open) {
        setOpen(false)
        setHighlight(-1)
        onQueryChange(buffer)
      } else if (hasContent) clearAll()
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
      if (!open && suggestionActivation === 'focus') {
        e.preventDefault()
        onQuerySubmit(buffer)
        return
      }
      if (suggestionActivation === 'intentional' && !expanded) {
        unhandled(e)
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
    unhandled(e)
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
    feedback:
      open && typed?.kind === 'token'
        ? t(
            parsed.kind !== 'token'
              ? 'history.composite.literalFilter'
              : completion.candidates.length === 0
                ? 'history.composite.noFilterOptions'
                : 'history.composite.chooseFilter',
            { dimension: t(DIMENSION_LABEL_KEYS[typed.dimension]) }
          )
        : null,
    changeSelection,
    changeText,
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
