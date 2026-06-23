import { Filter, filterToContentTypes } from '@/api/clipboardItems'
import type { SearchResultDto } from '@/api/daemon/search'
import { useClipboardSearch } from '@/hooks/useClipboardSearch'
import type { DisplayItem, TimeRangePreset } from '../types'

/** Map backend contentType to frontend display type. */
function mapContentTypeToDisplayType(ft: SearchResultDto['contentType']): DisplayItem['type'] {
  switch (ft) {
    case 'text':
      return 'text'
    case 'html':
      return 'code'
    case 'link':
      return 'link'
    case 'file':
      return 'file'
    case 'image':
      return 'image'
    case 'other':
      return 'unknown'
  }
}

function searchResultToDisplayItem(r: SearchResultDto): DisplayItem {
  return {
    id: r.entryId,
    type: mapContentTypeToDisplayType(r.contentType),
    preview: r.textPreview ?? '',
    activeTime: r.activeTimeMs,
    // TODO: SearchResultDto 暂不返回 payload_state, 搜索结果里 Lost 的 entry
    // 不会灰显。等后端搜索 API 透出 payload_state 后再接通。
    isUnavailable: false,
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
  if (tokenContentTypes.length > 0) {
    contentTypes = tokenContentTypes.join(',')
  } else if (!isAdvancedMode) {
    contentTypes = filterToContentTypes(activeFilter)
  }

  const { results, isSearching, total } = useClipboardSearch(
    {
      enabled: needsServerSearch,
      query: queryString,
      contentTypes,
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
