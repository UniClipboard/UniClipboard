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

interface UseHistorySearchProps {
  searchQuery: string
  activeFilter: Filter
  tagFilter: string | null
  sourceFilter: string | null
  extensionFilter: string | null
  timeRange: TimeRangePreset
}

interface UseHistorySearchResult {
  filteredItems: DisplayItem[]
  previewItems: DisplayClipboardItem[]
  /** A narrowed view (query/token/time/filter) is loading. Drives the spinner. */
  isSearching: boolean
  searchTotal: number | null
  /** Plain browse is loading (no narrowing yet). */
  loading: boolean
  isLocked: boolean
  /** Optimistically drop an entry after the user deletes it. */
  removeItem: (id: string) => void
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
  activeFilter,
  tagFilter,
  sourceFilter,
  extensionFilter,
  timeRange,
}: UseHistorySearchProps): UseHistorySearchResult {
  const { t } = useTranslation()
  const { isLocked } = useEncryptionSessionState()

  const trimmedQuery = searchQuery.trim()
  const tags = [filterToTags(activeFilter), tagFilter].filter(Boolean).join(',') || undefined

  const model = useMemo<LiveSearchQueryModel>(
    () => ({
      query: trimmedQuery,
      contentTypes: filterToContentTypes(activeFilter),
      tags,
      sourceDevices: sourceFilter ?? undefined,
      extensions: extensionFilter ?? undefined,
      // TimeRangePreset values match the backend timePreset directly.
      timeRange,
    }),
    [trimmedQuery, activeFilter, tags, sourceFilter, extensionFilter, timeRange]
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
    timeRange !== 'all_time' ||
    activeFilter !== Filter.All ||
    tagFilter !== null ||
    sourceFilter !== null ||
    extensionFilter !== null

  return {
    filteredItems,
    previewItems: live.items,
    isSearching: live.isLoading && narrowed,
    searchTotal: live.total,
    loading: live.isLoading && !narrowed,
    isLocked,
    removeItem: live.removeItem,
  }
}
