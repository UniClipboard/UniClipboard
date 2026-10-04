import type { Filter } from '@/api/clipboardItems'
import type { SearchTagOption } from '@/lib/search-tags'

/** Router state key carrying a Library row picked outside the History page. */
export const HISTORY_LIBRARY_FILTER_STATE = 'historyLibraryFilter'

/** The page filters a sidebar row can pick: the Library rows, plus the Tags
 * section's `file` row (a content type, not a tag). */
export type SidebarLibraryFilter = Filter.All | Filter.Favorited | Filter.File

/** Router state key carrying a Tags row picked outside the History page. */
export const HISTORY_TAG_FILTER_STATE = 'historyTagFilter'

/** On History the Library and Tags rows drive the page's own filter; on
 * Devices they navigate back to History with the choice in router state. */
export type HistorySidebarProps =
  | {
      context: 'history'
      activeFilter: Filter
      onSelectLibrary: (filter: SidebarLibraryFilter) => void
      tags: SearchTagOption[]
      activeTag: string | null
      onSelectTag: (tag: string | null) => void
      /** Changes whenever entries are added, removed or pinned, so the
       * Library counts refresh; History passes its item list. */
      countsRevision: unknown
    }
  | { context: 'devices'; tags: SearchTagOption[] }
