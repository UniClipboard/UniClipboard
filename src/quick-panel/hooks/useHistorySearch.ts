import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter, filterToContentTypes, filterToTags } from '@/api/clipboardItems'
import { type LiveSearchQueryModel } from '@/hooks/liveSearchModel'
import { useEncryptionSessionState } from '@/hooks/useEncryptionSessionState'
import { useLiveSearch } from '@/hooks/useLiveSearch'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import type { DisplayItem, TimeRangePreset } from '../types'

/** Quick-panel list cap. The launcher has no infinite scroll, so this is the
 * hard ceiling rather than a growing window (matches the old browse/search). */
const PAGE_SIZE = 50

/**
 * Derive the single-line preview the launcher rows show. The unified live list
 * carries structured `content` (the search index drops image dimensions — those
 * are LAZY, resolved by entry id — so an image row falls back to a localized
 * "Image" label instead of a dimension string).
 */
function panelPreview(item: DisplayClipboardItem, imageLabel: string): string {
  const c = item.content
  if (c) {
    if ('urls' in c) return c.urls[0] ?? ''
    if ('file_names' in c) return c.file_names[0] ?? ''
    if ('code' in c) return c.code
    if ('display_text' in c) return c.display_text
  }
  if (item.type === 'image') return item.textPreview?.trim() || imageLabel
  return item.textPreview ?? ''
}

function toDisplayItem(item: DisplayClipboardItem, imageLabel: string): DisplayItem {
  return {
    id: item.id,
    type: item.type,
    preview: panelPreview(item, imageLabel),
    activeTime: item.activeTime,
    isUnavailable: item.isUnavailable ?? false,
  }
}

/** The `type:` token vocabulary mirrors the quick-filter chips ({@link Filter}),
 * so each value resolves through the same content-type/tag split the chips use. */
const TYPE_TOKEN_TO_FILTER: Record<string, Filter> = {
  text: Filter.Text,
  image: Filter.Image,
  link: Filter.Link,
  code: Filter.Code,
  file: Filter.File,
  favorited: Filter.Favorited,
}

/**
 * Map a `type:<value>` token onto the backend search contract. Known values are
 * routed through {@link filterToContentTypes}/{@link filterToTags} so the token
 * DSL and the chips stay on one contract: `link`/`image`/`favorited` are tags,
 * `code` is the `html` content type. Unknown values pass through as a raw
 * content type so the backend can still match or reject them.
 */
function classifyTypeToken(value: string): { contentType?: string; tag?: string } {
  const filter = TYPE_TOKEN_TO_FILTER[value]
  if (filter) {
    return { contentType: filterToContentTypes(filter), tag: filterToTags(filter) }
  }
  return { contentType: value }
}

/**
 * Extract search parameters from advanced mode tokens.
 * Tokens can be: `type:text`, `ext:md`, or plain keywords. `type:` values share
 * the quick-filter vocabulary and are normalized into the tag/content-type
 * contract (see {@link classifyTypeToken}).
 */
function parseTokens(tokens: string[]): {
  keywords: string[]
  contentTypes: string[]
  tags: string[]
  extensions: string[]
} {
  const keywords: string[] = []
  const contentTypes: string[] = []
  const tags: string[] = []
  const extensions: string[] = []

  for (const token of tokens) {
    const lower = token.toLowerCase()
    if (lower.startsWith('type:')) {
      const value = lower.slice(5)
      if (value) {
        const { contentType, tag } = classifyTypeToken(value)
        if (contentType) contentTypes.push(contentType)
        if (tag) tags.push(tag)
      }
    } else if (lower.startsWith('ext:')) {
      const value = lower.slice(4)
      if (value) extensions.push(value)
    } else if (token.trim()) {
      keywords.push(token.trim())
    }
  }

  return { keywords, contentTypes, tags, extensions }
}

interface UseHistorySearchProps {
  searchQuery: string
  tokens: string[]
  activeFilter: Filter
  timeRange: TimeRangePreset
  isAdvancedMode: boolean
}

interface UseHistorySearchResult {
  filteredItems: DisplayItem[]
  /** A narrowed view (query/token/time/filter) is loading. Drives the spinner. */
  isSearching: boolean
  searchTotal: number | null
  /** Plain browse is loading (no narrowing yet). */
  loading: boolean
  isLocked: boolean
  /** Optimistically drop an entry after the user deletes it. */
  removeItem: (id: string) => void
  /** Re-issue the current query (used to refresh on every panel re-open). */
  refetch: () => void
}

/**
 * Quick-panel data layer: browse and search are one engine path via
 * {@link useLiveSearch} (an empty query browses, any filter narrows), replacing
 * the old split of `useClipboardCollection` (list endpoint) plus a separate
 * server-search executor. Returns the launcher's simplified {@link DisplayItem}
 * view model.
 */
export function useHistorySearch({
  searchQuery,
  tokens,
  activeFilter,
  timeRange,
  isAdvancedMode,
}: UseHistorySearchProps): UseHistorySearchResult {
  const { t } = useTranslation()
  const { isLocked } = useEncryptionSessionState()

  // Build the query model from tokens + free text + filter/time controls.
  const {
    keywords,
    contentTypes: tokenContentTypes,
    tags: tokenTags,
    extensions,
  } = parseTokens(tokens)
  const trimmedQuery = searchQuery.trim()
  const queryString = (trimmedQuery ? [...keywords, trimmedQuery] : keywords).join(' ')
  // Pre-join to a stable string so the memo deps stay primitive (the array would
  // be a fresh reference every render and re-issue the query in a loop).
  const extensionsStr = extensions.length > 0 ? extensions.join(',') : undefined

  // Tokens win; otherwise (non-advanced) fall back to the quick filter. Tokens
  // now carry both dimensions (`type:link`/`type:image` resolve to tags), so a
  // token in either bucket suppresses the chip fallback.
  let contentTypes: string | undefined
  let tags: string | undefined
  if (tokenContentTypes.length > 0 || tokenTags.length > 0) {
    contentTypes = tokenContentTypes.length > 0 ? tokenContentTypes.join(',') : undefined
    tags = tokenTags.length > 0 ? tokenTags.join(',') : undefined
  } else if (!isAdvancedMode) {
    contentTypes = filterToContentTypes(activeFilter)
    tags = filterToTags(activeFilter)
  }

  const model = useMemo<LiveSearchQueryModel>(
    () => ({
      query: queryString,
      contentTypes,
      tags,
      extensions: extensionsStr,
      // TimeRangePreset values match the backend timePreset directly.
      timeRange,
    }),
    [queryString, contentTypes, tags, extensionsStr, timeRange]
  )

  const live = useLiveSearch({ model, pageSize: PAGE_SIZE })

  const imageLabel = t('history.type.image')
  const filteredItems = useMemo(
    () => live.items.map(item => toDisplayItem(item, imageLabel)),
    [live.items, imageLabel]
  )

  // A narrowed view (keyword, advanced token, time filter, or content filter)
  // shows the "searching" spinner; plain browse shows the "loading" spinner.
  const narrowed =
    trimmedQuery.length > 0 ||
    tokens.length > 0 ||
    timeRange !== 'all_time' ||
    activeFilter !== Filter.All

  return {
    filteredItems,
    isSearching: live.isLoading && narrowed,
    searchTotal: live.total,
    loading: live.isLoading && !narrowed,
    isLocked,
    removeItem: live.removeItem,
    refetch: live.refetch,
  }
}
