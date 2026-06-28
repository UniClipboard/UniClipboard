/**
 * Composite-search model — pure types and helpers shared by the chip input.
 *
 * This module owns the *vocabulary* of the composite search box: the filter
 * dimensions (content type / source device / time range), how a raw input
 * buffer parses into either a free-text query or a `key:value` token, and how
 * the current filter state projects into renderable chips and suggestion
 * candidates.
 *
 * Scope (per the UI-only redesign): the box is a thin skin over the History
 * page's four existing state values. This file produces *data*; the React
 * components own focus, keyboard handling, and applying values back to state.
 */
import {
  Clock,
  Code,
  ExternalLink,
  File,
  FileText,
  Hash,
  Image as ImageIcon,
  Laptop,
  Smartphone,
  type LucideIcon,
} from 'lucide-react'
import { Filter } from '@/api/clipboardItems'
import type { SearchTagDto, TimeRangePreset } from '@/api/daemon/search'
import { mergeSearchTagOptions, type SearchTagOption } from '@/lib/search-tags'

/** A selectable source device (P2P space member or mobile-sync device). */
export interface SourceOption {
  id: string
  name: string
  kind: 'p2p' | 'mobile'
}

/** The filter dimensions the box can build. */
export type Dimension = 'type' | 'tag' | 'source' | 'time'

/**
 * English syntax-key prefix typed by keyboard users (decision: fixed English
 * keys, not localized). `source` reads `from:` to match common filter-bar
 * conventions; the others mirror their dimension name.
 */
export const SYNTAX_KEYS: Record<Dimension, string> = {
  type: 'type',
  tag: '#',
  source: 'from',
  time: 'time',
}

/** Reverse map: typed prefix (lowercased) -> dimension. */
const PREFIX_TO_DIMENSION: Record<string, Dimension> = {
  type: 'type',
  from: 'source',
  time: 'time',
}

/** Physical content-type filters offered as `type:` candidates. */
const TYPE_FILTERS: readonly Filter[] = [Filter.Text, Filter.Image, Filter.File]

/** Time presets offered as `time:` candidates (`all_time` == no filter, excluded). */
const TIME_PRESETS: readonly TimeRangePreset[] = [
  'today',
  'yesterday',
  'last_7d',
  'last_30d',
  'this_week',
  'this_month',
]

/** Per-content-type glyphs, mirroring the old filter-tab icons. */
const TYPE_ICONS: Record<string, LucideIcon> = {
  [Filter.Text]: FileText,
  [Filter.Code]: Code,
  [Filter.Link]: ExternalLink,
  [Filter.Image]: ImageIcon,
  [Filter.File]: File,
}

/** Default ("cleared") value of each dimension — clearing a chip resets to this. */
export const DIMENSION_DEFAULTS = {
  type: Filter.All,
  tag: null,
  source: null,
  time: 'all_time' as TimeRangePreset,
} as const

/** State setters a dimension value dispatches into (the History page's state). */
export interface DimensionHandlers {
  onContentFilterChange: (filter: Filter) => void
  onTagFilterChange: (tag: string | null) => void
  onSourceFilterChange: (id: string | null) => void
  onTimeRangeChange: (preset: TimeRangePreset) => void
}

/** Apply a raw candidate value to its dimension's state. Single dispatch point
 * shared by the chip input and the quick filter bar so they can't drift. */
export function applyDimensionValue(
  dimension: Dimension,
  value: string,
  h: DimensionHandlers
): void {
  if (dimension === 'type') h.onContentFilterChange(value as Filter)
  else if (dimension === 'tag') h.onTagFilterChange(value)
  else if (dimension === 'source') h.onSourceFilterChange(value)
  else h.onTimeRangeChange(value as TimeRangePreset)
}

/** Reset a dimension to its default (no filter). */
export function resetDimensionValue(dimension: Dimension, h: DimensionHandlers): void {
  if (dimension === 'type') h.onContentFilterChange(DIMENSION_DEFAULTS.type)
  else if (dimension === 'tag') h.onTagFilterChange(DIMENSION_DEFAULTS.tag)
  else if (dimension === 'source') h.onSourceFilterChange(DIMENSION_DEFAULTS.source)
  else h.onTimeRangeChange(DIMENSION_DEFAULTS.time)
}

// ── Buffer parsing ──────────────────────────────────────────────

export type ParsedBuffer =
  | { kind: 'query'; text: string }
  | { kind: 'token'; dimension: Dimension; partial: string; committed: boolean }

/**
 * Classify the live input buffer.
 *
 * A buffer is a *token* only when (after leading whitespace) it starts with a
 * known `key:` prefix — e.g. `type:image`. Trailing whitespace after the value
 * marks it ready to commit into a chip. Anything else (including text with an
 * unrelated colon like a URL) is free-text query. Keeping tokens anchored to
 * the buffer start lets a query and chips coexist without ambiguous parsing.
 */
export function parseBuffer(buffer: string): ParsedBuffer {
  const lead = buffer.replace(/^\s+/, '')
  const tagMatch = /^#([\s\S]*)$/.exec(lead)
  if (tagMatch) {
    const rest = tagMatch[1]
    const committed = /\s$/.test(rest) && rest.trim().length > 0
    return { kind: 'token', dimension: 'tag', partial: rest.trim(), committed }
  }
  const match = /^([a-zA-Z]+):([\s\S]*)$/.exec(lead)
  if (match) {
    const dimension = PREFIX_TO_DIMENSION[match[1].toLowerCase()]
    if (dimension) {
      const rest = match[2]
      const committed = /\s$/.test(rest) && rest.trim().length > 0
      return { kind: 'token', dimension, partial: rest.trim(), committed }
    }
  }
  return { kind: 'query', text: buffer }
}

// ── Candidates & chips ──────────────────────────────────────────

export interface CandidateItem {
  /** Stable id for React keys. */
  id: string
  dimension: Dimension
  /** Raw value applied to state: Filter for type, device id for source, preset for time. */
  value: string
  label: string
  icon: LucideIcon
  /** Whether this value is the dimension's current selection. */
  isActive: boolean
}

export function searchableTagsToOptions(tags: SearchTagDto[]): SearchTagOption[] {
  return mergeSearchTagOptions(tags)
}

export interface ChipData {
  dimension: Dimension
  label: string
  icon: LucideIcon
}

type Translate = (key: string, opts?: Record<string, unknown>) => string

/** Current selection snapshot, mirrored from the History page's state. */
export interface FilterSnapshot {
  type: Filter
  tag: string | null
  source: string | null
  time: TimeRangePreset
}

/** i18n keys for each dimension's group header in the suggestion panel. */
export const DIMENSION_LABEL_KEYS: Record<Dimension, string> = {
  type: 'history.composite.dimension.type',
  tag: 'history.composite.dimension.tag',
  source: 'history.composite.dimension.source',
  time: 'history.composite.dimension.time',
}

const DIMENSION_ICONS: Record<Dimension, LucideIcon> = {
  type: FileText,
  tag: Hash,
  source: Laptop,
  time: Clock,
}

export interface SyntaxSuggestion {
  dimension: Dimension
  /** Dimension display name, e.g. 类型. */
  label: string
  /** The syntax seed to surface/apply, e.g. `type:`. */
  hint: string
  icon: LucideIcon
}

/**
 * Syntax-prefix hints. Typing `t` suggests `type:` / `time:`; `f` suggests
 * `from:`. Keeps the keyboard token syntax discoverable now that the panel
 * shows flat values instead of explicit dimension entries. Matches any
 * dimension whose syntax key starts with the typed text.
 */
export function buildSyntaxSuggestions(partial: string, t: Translate): SyntaxSuggestion[] {
  const needle = partial.trimStart().toLowerCase()
  if (!needle) return []
  return (['type', 'tag', 'source', 'time'] as const).flatMap(dimension =>
    dimension === 'tag' && needle !== '#'
      ? []
      : SYNTAX_KEYS[dimension].startsWith(needle)
        ? [
            {
              dimension,
              label: t(DIMENSION_LABEL_KEYS[dimension]),
              hint: dimension === 'tag' ? SYNTAX_KEYS[dimension] : `${SYNTAX_KEYS[dimension]}:`,
              icon: DIMENSION_ICONS[dimension],
            },
          ]
        : []
  )
}

function matches(partial: string, ...haystacks: string[]): boolean {
  if (!partial) return true
  const needle = partial.toLowerCase()
  return haystacks.some(h => h.toLowerCase().includes(needle))
}

/** Candidate values for one dimension, narrowed by the typed `partial`. */
export function buildCandidates(
  dimension: Dimension,
  partial: string,
  ctx: {
    t: Translate
    sourceOptions: SourceOption[]
    current: FilterSnapshot
    tagOptions: SearchTagOption[]
  }
): CandidateItem[] {
  switch (dimension) {
    case 'type':
      return TYPE_FILTERS.flatMap(filter => {
        const label = ctx.t(`history.type.${filter}`)
        return matches(partial, filter, label)
          ? [
              {
                id: `cand-type-${filter}`,
                dimension,
                value: filter,
                label,
                icon: TYPE_ICONS[filter] ?? FileText,
                isActive: ctx.current.type === filter,
              },
            ]
          : []
      })
    case 'tag':
      return ctx.tagOptions.flatMap(tag => {
        const label = ctx.t(`history.type.${tag.id}`, { defaultValue: tag.id })
        return matches(partial, tag.id, label)
          ? [
              {
                id: `cand-tag-${tag.id}`,
                dimension,
                value: tag.id,
                label,
                icon: TYPE_ICONS[tag.id] ?? Hash,
                isActive: ctx.current.tag === tag.id,
              },
            ]
          : []
      })
    case 'source':
      return ctx.sourceOptions.flatMap(opt =>
        matches(partial, opt.name, opt.id)
          ? [
              {
                id: `cand-source-${opt.id}`,
                dimension,
                value: opt.id,
                label: opt.name,
                icon: opt.kind === 'mobile' ? Smartphone : Laptop,
                isActive: ctx.current.source === opt.id,
              },
            ]
          : []
      )
    case 'time':
      return TIME_PRESETS.flatMap(preset => {
        const label = ctx.t(`history.timeRange.${preset}`)
        return matches(partial, preset, label)
          ? [
              {
                id: `cand-time-${preset}`,
                dimension,
                value: preset,
                label,
                icon: Clock,
                isActive: ctx.current.time === preset,
              },
            ]
          : []
      })
  }
}

/**
 * All filter values across every dimension, narrowed by `partial`, in stable
 * dimension order (type → source → time). This flat list powers the default
 * panel: focusing the box surfaces every selectable value directly (one
 * keystroke / arrow-key away), with no intermediate "pick a category" step.
 */
export function buildAllCandidates(
  partial: string,
  ctx: {
    t: Translate
    sourceOptions: SourceOption[]
    current: FilterSnapshot
    tagOptions: SearchTagOption[]
  }
): CandidateItem[] {
  return [
    ...buildCandidates('type', partial, ctx),
    ...buildCandidates('tag', partial, ctx),
    ...buildCandidates('source', partial, ctx),
    ...buildCandidates('time', partial, ctx),
  ]
}

/** Active filters projected to chips, in stable dimension order. */
export function buildChips(ctx: {
  t: Translate
  sourceOptions: SourceOption[]
  current: FilterSnapshot
  tagOptions: SearchTagOption[]
}): ChipData[] {
  const chips: ChipData[] = []
  const { type, tag, source, time } = ctx.current
  if (type !== Filter.All && type !== Filter.Favorited) {
    chips.push({
      dimension: 'type',
      label: ctx.t(`history.type.${type}`),
      icon: TYPE_ICONS[type] ?? FileText,
    })
  }
  if (tag !== null) {
    const opt = ctx.tagOptions.find(o => o.id === tag)
    chips.push({
      dimension: 'tag',
      label: ctx.t(`history.type.${tag}`, { defaultValue: tag }),
      icon: TYPE_ICONS[opt?.id ?? tag] ?? Hash,
    })
  }
  if (source !== null) {
    const opt = ctx.sourceOptions.find(o => o.id === source)
    chips.push({
      dimension: 'source',
      label: opt?.name ?? ctx.t('history.source.label'),
      icon: opt?.kind === 'mobile' ? Smartphone : Laptop,
    })
  }
  if (time !== 'all_time') {
    chips.push({
      dimension: 'time',
      label: ctx.t(`history.timeRange.${time}`),
      icon: Clock,
    })
  }
  return chips
}
