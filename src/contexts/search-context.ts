import { createContext, useContext } from 'react'

export type TimeRangePreset =
  | 'all_time'
  | 'today'
  | 'yesterday'
  | 'last_7d'
  | 'last_30d'
  | 'this_week'
  | 'this_month'

export interface SearchContextType {
  searchValue: string
  setSearchValue: (value: string) => void
  timeRange: TimeRangePreset
  setTimeRange: (range: TimeRangePreset) => void
}

export const SearchContext = createContext<SearchContextType | undefined>(undefined)

export const useSearch = () => {
  const context = useContext(SearchContext)
  if (context === undefined) {
    throw new Error('useSearch must be used within a SearchProvider')
  }
  return context
}
