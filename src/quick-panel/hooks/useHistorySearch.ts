import { Filter, filterToContentTypes, filterToTags } from '@/api/clipboardItems'
import type { SearchResultDto } from '@/api/daemon/search'
import { useClipboardSearch } from '@/hooks/useClipboardSearch'
import type { DisplayItem, TimeRangePreset } from '../types'

/** Map backend contentType to frontend display type. `link` lives in the tag
 * dimension, not here. */
function mapContentTypeToDisplayType(ft: SearchResultDto['contentType']): DisplayItem['type'] {
  switch (ft) {
    case 'text':
      return 'text'
    case 'html':
      return 'code'
    case 'file':
      return 'file'
    case 'image':
      return 'image'
    case 'other':
      return 'unknown'
  }
}

function searchResultToDisplayItem(r: SearchResultDto): DisplayItem {
  // `link` is a derived tag: a text entry carrying web URLs shows as a link.
  let type = mapContentTypeToDisplayType(r.contentType)
  if (type === 'text' && r.linkUrls.length > 0) type = 'link'
  return {
    id: r.entryId,
    type,
    preview: r.textPreview ?? '',
    activeTime: r.activeTimeMs,
    isUnavailable: r.payloadState === 'Lost',
  }
}

/**
 * Extract search parameters from advanced mode tokens.
 * Tokens can be: `type:text`, `ext:md`, or plain keywords.
 * `type:` values are passed directly as backend contentTypes (no mapping needed).
 */
function parseTokens(tokens: string[]): {
  keywords: string[]
  contentTypes: string[]
  extensions: string[]
} {
  const keywords: string[] = []
  const contentTypes: string[] = []
  const extensions: string[] = []

  for (const token of tokens) {
    const lower = token.toLowerCase()
    if (lower.startsWith('type:')) {
      const value = lower.slice(5)
      // code includes html (html is a form of code)
      if (value === 'code') {
        contentTypes.push('code', 'html')
      } else if (value) contentTypes.push(value)
    } else if (lower.startsWith('ext:')) {
      const value = lower.slice(4)
      if (value) extensions.push(value)
    } else if (token.trim()) {
      keywords.push(token.trim())
    }
  }

  return { keywords, contentTypes, extensions }
}

interface UseHistorySearchProps {
  items: DisplayItem[]
  searchQuery: string
  tokens: string[]
  activeFilter: Filter
  timeRange: TimeRangePreset
  isAdvancedMode: boolean
}

interface UseHistorySearchResult {
  filteredItems: DisplayItem[]
  isSearching: boolean
  searchTotal: number | null
}

export function useHistorySearch({
  items,
  searchQuery,
  tokens,
  activeFilter,
  timeRange,
  isAdvancedMode,
}: UseHistorySearchProps): UseHistorySearchResult {
  // Determine if we need to call the server
  const hasQuery = searchQuery.trim().length > 0
  const hasTokens = tokens.length > 0
  const hasTimeFilter = timeRange !== 'all_time'
  const needsServerSearch = hasQuery || hasTokens || hasTimeFilter

  // Build the query model from tokens + free text + filter/time controls.
  const { keywords, contentTypes: tokenContentTypes, extensions } = parseTokens(tokens)
  const trimmedQuery = searchQuery.trim()
  const queryString = (trimmedQuery ? [...keywords, trimmedQuery] : keywords).join(' ')

  // contentTypes: tokens win; otherwise (non-advanced) fall back to the filter.
  let contentTypes: string | undefined
  let tags: string | undefined
  if (tokenContentTypes.length > 0) {
    contentTypes = tokenContentTypes.join(',')
  } else if (!isAdvancedMode) {
    contentTypes = filterToContentTypes(activeFilter)
    tags = filterToTags(activeFilter)
  }

  const { results, isSearching, total } = useClipboardSearch(
    {
      enabled: needsServerSearch,
      query: queryString,
      contentTypes,
      tags,
      extensions: extensions.length > 0 ? extensions.join(',') : undefined,
      // TimeRangePreset values match backend timePreset directly ('all_time' → omit).
      timePreset: timeRange !== 'all_time' ? timeRange : undefined,
      limit: 50,
    },
    searchResultToDisplayItem
  )

  // Apply local filter only (no search query, no advanced tokens, no time filter)
  const localFilteredItems = (() => {
    if (needsServerSearch) return items // won't be used
    if (activeFilter === Filter.All) return items
    const typeMap: Record<string, string> = {
      [Filter.Text]: 'text',
      [Filter.Image]: 'image',
      [Filter.Link]: 'link',
      [Filter.File]: 'file',
      [Filter.Code]: 'code',
    }
    const target = typeMap[activeFilter]
    return target ? items.filter(item => item.type === target) : items
  })()

  return {
    filteredItems: needsServerSearch ? (results ?? items) : localFilteredItems,
    isSearching,
    searchTotal: total,
  }
}
