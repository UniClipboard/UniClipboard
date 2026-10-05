import type { Filter } from '@/api/clipboardItems'
import type { SearchTagOption } from '@/lib/search-tags'

/** Router state key carrying a Library row picked outside the History page. */
export const HISTORY_LIBRARY_FILTER_STATE = 'historyLibraryFilter'

/** The page filters a sidebar Library row can pick. */
export type SidebarLibraryFilter = Filter.All | Filter.Favorited

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
      /** The tags the sidebar shows, in order (the daemon's tag layout);
       * `null` while it is unknown (loading, locked). */
      sidebarTagIds: readonly string[] | null
      activeTag: string | null
      onSelectTag: (tag: string | null) => void
      /** The tag Library: how many tags it lists, and how to open it —
       * `returnFocusTo` is the control to refocus on close, or `null` to
       * leave focus alone. Absent while local tags are unavailable. */
      tagLibrary?: { total: number; open: (returnFocusTo: HTMLElement | null) => void }
      /** Changes whenever entries are added, removed or pinned, so the
       * Library counts refresh; History passes its item list. */
      countsRevision: unknown
    }
  | { context: 'devices'; tags: SearchTagOption[]; sidebarTagIds: readonly string[] | null }
