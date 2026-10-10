import { useMemo } from 'react'
import type { HistoryTagLayoutDto } from '@/api/daemon/history-tags'
import { useHistoryTags, type HistoryTagsState } from '@/hooks/useHistoryTags'
import { useSearchTags } from '@/hooks/useSearchTags'
import { useTagLayout } from '@/hooks/useTagLayout'
import { withTagNames, type SearchTagOption } from '@/lib/search-tags'

export interface TagCatalog {
  /** Searchable tags with counts; local tags carry their names. */
  searchableTags: SearchTagOption[]
  /** This device's local tags (names, association counts, availability). */
  historyTags: HistoryTagsState
  /** The sidebar's tags in order and every tag's color; `null` while unknown. */
  layout: HistoryTagLayoutDto | null
}

/** Every tag surface's one source: `/search/tags` counts joined with the names
 * from `/history/tags`, and the daemon's tag layout. Bump `revision` after a
 * tag change to refetch all three. */
export function useTagCatalog(revision = 0): TagCatalog {
  const searchTags = useSearchTags(revision)
  const historyTags = useHistoryTags(revision)
  const layout = useTagLayout(revision)
  const searchableTags = useMemo(
    () => withTagNames(searchTags, historyTags.tags),
    [searchTags, historyTags.tags]
  )
  return { searchableTags, historyTags, layout }
}
