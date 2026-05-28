import { useCallback, useMemo, useState, type ReactNode } from 'react'
import { SearchContext, type TimeRangePreset } from './search-context'

export const SearchProvider = ({ children }: { children: ReactNode }) => {
  const [searchValue, setSearchValue] = useState('')
  const [timeRange, setTimeRange] = useState<TimeRangePreset>('all_time')

  const handleSetSearchValue = useCallback((value: string) => {
    setSearchValue(value)
  }, [])

  const handleSetTimeRange = useCallback((range: TimeRangePreset) => {
    setTimeRange(range)
  }, [])

  const contextValue = useMemo(
    () => ({
      searchValue,
      setSearchValue: handleSetSearchValue,
      timeRange,
      setTimeRange: handleSetTimeRange,
    }),
    [searchValue, timeRange, handleSetSearchValue, handleSetTimeRange]
  )

  return <SearchContext.Provider value={contextValue}>{children}</SearchContext.Provider>
}
