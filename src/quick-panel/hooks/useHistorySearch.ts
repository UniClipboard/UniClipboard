import { useCallback, useEffect, useRef, useState } from 'react'
import type { DisplayItem, TimeRangePreset } from '../types'
import { Filter } from '@/api/clipboardItems'
import { querySearch } from '@/api/daemon/search'
import type { SearchResultDto, SearchParams } from '@/api/daemon/search'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-history-search')

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
  const [searchResults, setSearchResults] = useState<DisplayItem[] | null>(null)
  const [isSearching, setIsSearching] = useState(false)
  const [searchTotal, setSearchTotal] = useState<number | null>(null)
  const abortRef = useRef<AbortController | null>(null)

  // Determine if we need to call the server
  const hasQuery = searchQuery.trim().length > 0
  const hasTokens = tokens.length > 0
  const hasTimeFilter = timeRange !== 'all_time'
  const needsServerSearch = hasQuery || hasTokens || hasTimeFilter

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

  const doSearch = useCallback(
    async (
      query: string,
      advTokens: string[],
      filter: Filter,
      time: TimeRangePreset,
      advanced: boolean,
      signal: AbortSignal
    ) => {
      const { keywords, contentTypes: tokenFileTypes, extensions } = parseTokens(advTokens)

      // Build query string: combine free-text + keyword tokens
      const queryParts = [...keywords]
      const trimmed = query.trim()
      if (trimmed) queryParts.push(trimmed)
      const queryString = queryParts.join(' ')

      // Build contentTypes: from tokens or from active filter (values match backend directly)
      let contentTypes: string | undefined
      if (tokenFileTypes.length > 0) {
        contentTypes = tokenFileTypes.join(',')
      } else if (!advanced && filter !== Filter.All && filter !== Filter.Favorited) {
        // Filter enum values match backend contentTypes directly;
        // Code includes html (html is a form of code)
        contentTypes = filter === Filter.Code ? 'code,html' : filter
      }

      const params: SearchParams = {
        query: queryString,
        contentTypes,
        extensions: extensions.length > 0 ? extensions.join(',') : undefined,
        // TimeRangePreset values match backend timePreset directly (except 'all_time' → omit)
        timePreset: time !== 'all_time' ? time : undefined,
        limit: 50,
      }

      return querySearch(params, signal)
    },
    []
  )

  useEffect(() => {
    if (!needsServerSearch) {
      setSearchResults(null)
      setSearchTotal(null)
      setIsSearching(false)
      return
    }

    // Cancel previous request
    abortRef.current?.abort()
    const controller = new AbortController()
    abortRef.current = controller

    setIsSearching(true)

    doSearch(searchQuery, tokens, activeFilter, timeRange, isAdvancedMode, controller.signal)
      .then(response => {
        if (controller.signal.aborted) return
        setSearchResults(response.data.map(searchResultToDisplayItem))
        setSearchTotal(response.total)
        setIsSearching(false)
      })
      .catch(err => {
        if (controller.signal.aborted) return
        if (err instanceof DOMException && err.name === 'AbortError') return
        log.error({ err }, 'Search query failed')
        setSearchResults([])
        setSearchTotal(0)
        setIsSearching(false)
      })

    return () => {
      controller.abort()
    }
  }, [searchQuery, tokens, activeFilter, timeRange, isAdvancedMode, needsServerSearch, doSearch])

  return {
    filteredItems: needsServerSearch ? (searchResults ?? items) : localFilteredItems,
    isSearching,
    searchTotal,
  }
}
