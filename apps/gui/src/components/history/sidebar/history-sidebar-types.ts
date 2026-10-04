import type { Filter } from '@/api/clipboardItems'

/** Router state key carrying a Library row picked outside the History page. */
export const HISTORY_LIBRARY_FILTER_STATE = 'historyLibraryFilter'

/** On History the Library rows drive the page's own filter; on Devices they
 * navigate back to History with the chosen filter in router state. */
export type HistorySidebarProps =
  | {
      context: 'history'
      activeFilter: Filter
      onSelectLibrary: (filter: Filter.All | Filter.Favorited) => void
    }
  | { context: 'devices' }
