import { useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import type { TimeRangePreset } from '@/api/daemon/search'
import { readHistorySessionSnapshot } from '@/hooks/historySessionSnapshot'
import { useShortcut } from '@/hooks/useShortcut'
import type { SearchTagOption } from '@/lib/search-tags'
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
  type DimensionHandlers,
  type SourceOption,
} from './composite-search-model'
import type { PanelOption } from './SuggestionPanel'

export interface CompositeSearchBarProps {
  contentFilter: Filter
  sourceFilter: string | null
  tagFilter: string | null
  timeRange: TimeRangePreset
  onContentFilterChange: (filter: Filter) => void
  onTagFilterChange: (tag: string | null) => void
  onSourceFilterChange: (id: string | null) => void
  onTimeRangeChange: (preset: TimeRangePreset) => void
  onQueryChange: (text: string) => void
  onQuerySubmit: (text: string) => void
  sourceOptions: SourceOption[]
  tagOptions: SearchTagOption[]
  totalCount: number
  inputRef: React.RefObject<HTMLInputElement | null>
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
  onQueryChange,
  onQuerySubmit,
  sourceOptions,
  tagOptions,
  inputRef,
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
  const current = { type: contentFilter, tag: tagFilter, source: sourceFilter, time: timeRange }
  const chips = buildChips({ t, sourceOptions, tagOptions, current })
  const parsed = parseBuffer(buffer)
  const inToken = parsed.kind === 'token'
  const candidates: CandidateItem[] = inToken
    ? buildCandidates(parsed.dimension, parsed.partial, { t, sourceOptions, tagOptions, current })
    : buildAllCandidates(buffer, { t, sourceOptions, tagOptions, current })
  const syntaxSuggestions =
    inToken || buffer.trimStart().startsWith('#') ? [] : buildSyntaxSuggestions(buffer, t)
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
      header:
        !inToken && (i === 0 || candidates[i - 1].dimension !== c.dimension)
          ? t(DIMENSION_LABEL_KEYS[c.dimension])
          : undefined,
    })),
  ]
  const clampedHighlight =
    highlight < 0 || options.length === 0 ? -1 : Math.min(highlight, options.length - 1)
  const expanded = open && options.length > 0
  const handlers: DimensionHandlers = {
    onContentFilterChange,
    onTagFilterChange,
    onSourceFilterChange,
    onTimeRangeChange,
  }
  const resetDimension = (dimension: Dimension) => resetDimensionValue(dimension, handlers)

  const applyCandidate = (c: CandidateItem) => {
    applyDimensionValue(c.dimension, c.value, handlers)
    setBuffer('')
    onQueryChange('')
    setHighlight(-1)
    setOpen(true)
    inputRef.current?.focus()
  }

  const seedDimension = (dimension: Dimension) => {
    // The tag dimension's syntax key (`#`) is the whole prefix; the others take a
    // trailing colon (`type:`). Seeding `#:` would make `parseBuffer` treat `:`
    // as the partial tag text and surface no useful matches.
    setBuffer(dimension === 'tag' ? SYNTAX_KEYS.tag : `${SYNTAX_KEYS[dimension]}:`)
    setHighlight(-1)
    onQueryChange('')
    inputRef.current?.focus()
    setOpen(true)
  }

  const tryCommit = (dimension: Dimension, partial: string) => {
    const cands = buildCandidates(dimension, partial, { t, sourceOptions, tagOptions, current })
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
        selectOption(clampedHighlight)
      } else if (inToken) {
        if (candidates.length > 0) applyCandidate(candidates[0])
      } else {
        onQuerySubmit(buffer)
        setOpen(false)
      }
    }
  }

  useShortcut({
    key: 'esc',
    scope: 'clipboard',
    enabled: hasContent,
    handler: () => clearAll({ refocus: false }),
    enableOnFormTags: false,
  })

  return {
    t,
    buffer,
    open,
    setOpen,
    panelId,
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
    seedDimension,
    resetDimension,
    selectOption,
    setHighlight,
  }
}
