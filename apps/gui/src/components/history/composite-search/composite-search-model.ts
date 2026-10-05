/**
 * Composite-search model — pure types and helpers shared by the chip input.
 *
 * This module owns the *vocabulary* of the composite search box: the filter
 * dimensions (content type / source device / time range), how a raw input
 * buffer parses into either a free-text query or a filter token, and how
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
  FileCode,
  FileText,
  Folder,
  Hash,
  Image as ImageIcon,
  Laptop,
  Smartphone,
  type LucideIcon,
} from 'lucide-react'
import { Filter } from '@/api/clipboardItems'
import {
  MAX_SEARCH_COUNT_BATCH,
  type SearchParams,
  type SearchTagDto,
  type TimeRangePreset,
} from '@/api/daemon/search'
import { buildLiveSearchModel, liveModelToSearchParams } from '@/hooks/liveSearchModel'
import {
  mergeSearchTagOptions,
  splitSearchTags,
  tagLabel,
  toggleSearchTag,
  type SearchTagOption,
} from '@/lib/search-tags'

/** A selectable source device (P2P space member or mobile-sync device). */
export interface SourceOption {
  id: string
  name: string
  kind: 'p2p' | 'mobile'
}

/** The filter dimensions the box can build. */
export type Dimension = 'type' | 'tag' | 'source' | 'time' | 'extension'

/**
 * The prefix typed before a filter value (fixed, not localized). Tag, source
 * and type take the GPUI quick panel's one-character sigils (`#work`,
 * `@iPhone`, `/image`, see `crates/quick-panel-core/src/query/filters.rs`);
 * time and extension, which the quick panel has no sigil for, keep a
 * `key:` keyword.
 */
export const SYNTAX_KEYS: Record<Dimension, string> = {
  type: '/',
  tag: '#',
  source: '@',
  time: 'on:',
  extension: 'ext:',
}

const DIMENSIONS = Object.keys(SYNTAX_KEYS) as Dimension[]

/** Whether the dimension is typed with a one-character sigil rather than a `key:`. */
function isSigil(dimension: Dimension): boolean {
  return !SYNTAX_KEYS[dimension].endsWith(':')
}

/** Reverse maps: sigil -> dimension, and lowercase keyword (without `:`) -> dimension. */
const SIGIL_TO_DIMENSION = new Map(
  DIMENSIONS.filter(isSigil).map(dimension => [SYNTAX_KEYS[dimension], dimension])
)
const KEYWORD_TO_DIMENSION = new Map(
  DIMENSIONS.filter(dimension => !isSigil(dimension)).map(dimension => [
    SYNTAX_KEYS[dimension].slice(0, -1),
    dimension,
  ])
)

/** Physical content-type filters offered as `/` candidates. */
const TYPE_FILTERS: readonly Filter[] = [Filter.Text, Filter.RichText, Filter.Image, Filter.File]

/** Time presets offered as `on:` candidates (`all_time` == no filter, excluded). */
const TIME_PRESETS: readonly TimeRangePreset[] = [
  'today',
  'yesterday',
  'last_7d',
  'last_30d',
  'this_week',
  'this_month',
]

/** File extensions offered as `ext:` candidates. */
const EXTENSION_FILTERS = ['txt', 'md', 'jpg', 'png', 'pdf', 'ts', 'js', 'json', 'rs', 'go']

/** Per-content-type glyphs, mirroring the old filter-tab icons. */
const TYPE_ICONS: Record<string, LucideIcon> = {
  [Filter.Text]: FileText,
  [Filter.RichText]: FileText,
  [Filter.Code]: Code,
  [Filter.Link]: ExternalLink,
  [Filter.Image]: ImageIcon,
  [Filter.File]: File,
  directory: Folder,
}

/** Default ("cleared") value of each dimension — clearing a chip resets to this. */
export const DIMENSION_DEFAULTS = {
  type: Filter.All,
  tag: null,
  source: null,
  time: 'all_time' as TimeRangePreset,
  extension: null,
} as const

/** State setters a dimension value dispatches into (the History page's state). */
export interface DimensionHandlers {
  onContentFilterChange: (filter: Filter) => void
  onTagFilterChange: (tag: string | null) => void
  onSourceFilterChange: (id: string | null) => void
  onTimeRangeChange: (preset: TimeRangePreset) => void
  onExtensionFilterChange: (extension: string | null) => void
}

/** Apply a raw candidate value to its dimension's state. Single dispatch point
 * shared by the chip input and the quick filter bar so they can't drift. Tags
 * are multi-select: a tag value toggles in or out of the current selection. */
export function applyDimensionValue(
  dimension: Dimension,
  value: string,
  h: DimensionHandlers,
  current: FilterSnapshot
): void {
  if (dimension === 'type') h.onContentFilterChange(value as Filter)
  else if (dimension === 'tag') h.onTagFilterChange(toggleSearchTag(current.tag, value))
  else if (dimension === 'source') h.onSourceFilterChange(value)
  else if (dimension === 'time') h.onTimeRangeChange(value as TimeRangePreset)
  else h.onExtensionFilterChange(value)
}

/**
 * Render a dimension's current value back into typed-token syntax (e.g.
 * `/image`), so removing a chip via Backspace can drop the user back into
 * editing it instead of just clearing it.
 */
export function buildTokenText(dimension: Dimension, value: string): string {
  return `${SYNTAX_KEYS[dimension]}${value}`
}

/** Reset a dimension to its default (no filter). */
export function resetDimensionValue(dimension: Dimension, h: DimensionHandlers): void {
  if (dimension === 'type') h.onContentFilterChange(DIMENSION_DEFAULTS.type)
  else if (dimension === 'tag') h.onTagFilterChange(DIMENSION_DEFAULTS.tag)
  else if (dimension === 'source') h.onSourceFilterChange(DIMENSION_DEFAULTS.source)
  else if (dimension === 'time') h.onTimeRangeChange(DIMENSION_DEFAULTS.time)
  else h.onExtensionFilterChange(DIMENSION_DEFAULTS.extension)
}

// ── Buffer parsing ──────────────────────────────────────────────

export type ParsedBuffer =
  | { kind: 'query'; text: string }
  | { kind: 'token'; dimension: Dimension; partial: string; committed: boolean }

/**
 * Classify the live input buffer.
 *
 * A buffer is a *token* only when (after leading whitespace) it starts with a
 * known prefix — a sigil (`#work`, `@iPhone`, `/image`) or a `key:` keyword
 * (`on:today`). Trailing whitespace after the value marks it ready to commit
 * into a chip. Anything else (including text with an unrelated colon like a
 * URL) is free-text query. Keeping tokens anchored to the buffer start lets a
 * query and chips coexist without ambiguous parsing.
 */
export function parseBuffer(buffer: string): ParsedBuffer {
  const lead = buffer.replace(/^\s+/, '')
  const sigil = SIGIL_TO_DIMENSION.get(lead.charAt(0))
  const keyword = sigil ? null : /^([a-zA-Z]+):([\s\S]*)$/.exec(lead)
  const dimension = sigil ?? (keyword ? KEYWORD_TO_DIMENSION.get(keyword[1].toLowerCase()) : null)
  if (!dimension) return { kind: 'query', text: buffer }
  const rest = keyword ? keyword[2] : lead.slice(1)
  const committed = /\s$/.test(rest) && rest.trim().length > 0
  return { kind: 'token', dimension, partial: rest.trim(), committed }
}

/**
 * {@link parseBuffer}, except that a sigil word naming nothing stays text
 * search, as in the quick panel: `/tmp` or `@home` searches for that text
 * instead of dead-ending in a token with no candidates. A bare sigil still
 * opens its dimension's suggestions.
 */
export function resolveBuffer(buffer: string, ctx: CandidateContext): ParsedBuffer {
  const parsed = parseBuffer(buffer)
  return parsed.kind === 'token' &&
    isSigil(parsed.dimension) &&
    parsed.partial &&
    buildCandidates(parsed.dimension, parsed.partial, ctx).length === 0
    ? { kind: 'query', text: buffer }
    : parsed
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
  /** Values the chip holds: the tag chip carries the whole multi-tag selection. */
  valueCount: number
}

type Translate = (key: string, opts?: Record<string, unknown>) => string

/** Current selection snapshot, mirrored from the History page's state. */
export interface FilterSnapshot {
  type: Filter
  tag: string | null
  source: string | null
  time: TimeRangePreset
  extension: string | null
}

// ── Count queries ───────────────────────────────────────────────

function withDimension(
  current: FilterSnapshot,
  dimension: Dimension,
  value: string
): FilterSnapshot {
  if (dimension === 'type') return { ...current, type: value as Filter }
  if (dimension === 'time') return { ...current, time: value as TimeRangePreset }
  // Mirror `applyDimensionValue` so a count matches the list a click would show.
  if (dimension === 'tag') return { ...current, tag: toggleSearchTag(current.tag, value) }
  return { ...current, [dimension]: value }
}

function withoutDimension(current: FilterSnapshot, dimension: Dimension): FilterSnapshot {
  return { ...current, [dimension]: DIMENSION_DEFAULTS[dimension] }
}

function snapshotToSearchParams(snapshot: FilterSnapshot, query: string): SearchParams {
  return liveModelToSearchParams(
    buildLiveSearchModel({
      query,
      activeFilter: snapshot.type,
      tagFilter: snapshot.tag,
      sourceFilter: snapshot.source,
      extensionFilter: snapshot.extension,
      timeRange: snapshot.time,
    })
  )
}

/**
 * One count query per candidate: the current filters with that candidate
 * applied. Picking a candidate clears the text query, so the counts do too.
 * Capped at the daemon's batch size; later candidates get no count.
 */
export function buildCandidateCountQueries(
  candidates: CandidateItem[],
  current: FilterSnapshot
): SearchParams[] {
  return candidates
    .slice(0, MAX_SEARCH_COUNT_BATCH)
    .map(c => snapshotToSearchParams(withDimension(current, c.dimension, c.value), ''))
}

/** One count query per candidate on its own, ignoring the other filters: the
 * "12 items" before "· 4 from arch-desktop" in the list variant's hint. */
export function buildCandidateTotalQueries(candidates: CandidateItem[]): SearchParams[] {
  const none: FilterSnapshot = {
    type: DIMENSION_DEFAULTS.type,
    tag: DIMENSION_DEFAULTS.tag,
    source: DIMENSION_DEFAULTS.source,
    time: DIMENSION_DEFAULTS.time,
    extension: DIMENSION_DEFAULTS.extension,
  }
  return candidates
    .slice(0, MAX_SEARCH_COUNT_BATCH)
    .map(c => snapshotToSearchParams(withDimension(none, c.dimension, c.value), ''))
}

/** One count query per chip: the current search with only that chip removed. */
export function buildRelaxationQueries(
  chips: ChipData[],
  current: FilterSnapshot,
  query: string
): SearchParams[] {
  return chips.map(chip => snapshotToSearchParams(withoutDimension(current, chip.dimension), query))
}

/** i18n keys for each dimension's group header in the suggestion panel. */
export const DIMENSION_LABEL_KEYS: Record<Dimension, string> = {
  type: 'history.composite.dimension.type',
  tag: 'history.composite.dimension.tag',
  source: 'history.composite.dimension.source',
  time: 'history.composite.dimension.time',
  extension: 'history.composite.dimension.extension',
}

const DIMENSION_ICONS: Record<Dimension, LucideIcon> = {
  type: FileText,
  tag: Hash,
  source: Laptop,
  time: Clock,
  extension: FileCode,
}

export interface SyntaxSuggestion {
  dimension: Dimension
  /** Dimension display name, e.g. 类型. */
  label: string
  /** The syntax seed to surface/apply, e.g. `on:`. */
  hint: string
  icon: LucideIcon
}

/**
 * Keyword-prefix hints: typing `o` suggests `on:`, `e` suggests `ext:`. Keeps
 * the keyword syntax discoverable now that the panel shows flat values instead
 * of explicit dimension entries. Sigil dimensions need no hint — a typed sigil
 * already opens that dimension's values.
 */
export function buildSyntaxSuggestions(partial: string, t: Translate): SyntaxSuggestion[] {
  const needle = partial.trimStart().toLowerCase()
  if (!needle) return []
  return DIMENSIONS.filter(
    dimension => !isSigil(dimension) && SYNTAX_KEYS[dimension].startsWith(needle)
  ).map(dimension => ({
    dimension,
    label: t(DIMENSION_LABEL_KEYS[dimension]),
    hint: SYNTAX_KEYS[dimension],
    icon: DIMENSION_ICONS[dimension],
  }))
}

function matchRank(partial: string, ...haystacks: string[]): number | null {
  if (!partial) return 0
  const needle = partial.toLowerCase()
  if (haystacks.some(haystack => haystack.toLowerCase() === needle)) return 0
  if (haystacks.some(haystack => haystack.toLowerCase().startsWith(needle))) return 1
  if (haystacks.some(haystack => haystack.toLowerCase().includes(needle))) return 2
  return null
}

/** Rank 2 is a substring match; prefix-only matching drops it. */
const SUBSTRING_RANK = 2

/** Candidate values for one dimension, narrowed by the typed `partial`. */
export interface CandidateContext {
  t: Translate
  sourceOptions: SourceOption[]
  current: FilterSnapshot
  tagOptions: SearchTagOption[]
  /** Only values starting with `partial` (the list variant's "starting
   * with" suggestions); otherwise substrings match too, ranked last. */
  prefixOnly?: boolean
}

export function buildCandidates(
  dimension: Dimension,
  partial: string,
  ctx: CandidateContext
): CandidateItem[] {
  const rankOf = (...haystacks: string[]) => {
    const rank = matchRank(partial, ...haystacks)
    return rank === null || (ctx.prefixOnly && rank >= SUBSTRING_RANK) ? null : rank
  }
  const matches = (...haystacks: string[]) => rankOf(...haystacks) !== null
  const selectedTags = new Set(splitSearchTags(ctx.current.tag))
  switch (dimension) {
    case 'type':
      return TYPE_FILTERS.flatMap(filter => {
        const label = ctx.t(`history.type.${filter}`)
        return matches(filter, label)
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
      return ctx.tagOptions
        .flatMap((tag, index) => {
          const label = tagLabel(tag, ctx.t)
          const rank = rankOf(tag.id, label)
          return rank === null ? [] : [{ tag, label, rank, index }]
        })
        .sort((a, b) => a.rank - b.rank || b.tag.count - a.tag.count || a.index - b.index)
        .map(({ tag, label }) => ({
          id: `cand-tag-${tag.id}`,
          dimension,
          value: tag.id,
          label,
          icon: TYPE_ICONS[tag.id] ?? Hash,
          isActive: selectedTags.has(tag.id),
        }))
    case 'source':
      return ctx.sourceOptions.flatMap(opt =>
        matches(opt.name, opt.id)
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
        return matches(preset, label)
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
    case 'extension':
      return EXTENSION_FILTERS.flatMap(ext => {
        const label = `.${ext}`
        return matches(ext, label)
          ? [
              {
                id: `cand-extension-${ext}`,
                dimension,
                value: ext,
                label,
                icon: FileCode,
                isActive: ctx.current.extension === ext,
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
    ...buildCandidates('extension', partial, ctx),
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
  const { type, tag, source, time, extension } = ctx.current
  if (type !== Filter.All && type !== Filter.Favorited) {
    chips.push({
      dimension: 'type',
      label: ctx.t(`history.type.${type}`),
      icon: TYPE_ICONS[type] ?? FileText,
      valueCount: 1,
    })
  }
  if (tag !== null) {
    const opt = ctx.tagOptions.find(o => o.id === tag)
    const tags = splitSearchTags(tag)
    chips.push({
      dimension: 'tag',
      label: tags
        .map(id => `#${tagLabel(ctx.tagOptions.find(o => o.id === id) ?? { id }, ctx.t)}`)
        .join(', '),
      icon: TYPE_ICONS[opt?.id ?? tag] ?? Hash,
      valueCount: tags.length,
    })
  }
  if (source !== null) {
    const opt = ctx.sourceOptions.find(o => o.id === source)
    chips.push({
      dimension: 'source',
      label: opt?.name ?? ctx.t('history.source.label'),
      icon: opt?.kind === 'mobile' ? Smartphone : Laptop,
      valueCount: 1,
    })
  }
  if (time !== 'all_time') {
    chips.push({
      dimension: 'time',
      label: ctx.t(`history.timeRange.${time}`),
      icon: Clock,
      valueCount: 1,
    })
  }
  if (extension !== null) {
    chips.push({
      dimension: 'extension',
      label: `.${extension}`,
      icon: FileCode,
      valueCount: 1,
    })
  }
  return chips
}
