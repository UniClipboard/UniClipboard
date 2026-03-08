import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import ClipboardContent from '@/components/clipboard/ClipboardContent'
import Header from '@/components/layout/Header'
import { useSearch } from '@/contexts/search-context'
import { useClipboardEvents } from '@/hooks/useClipboardEvents'
import { useLifecycleStatus } from '@/hooks/useLifecycleStatus'
import { useShortcutScope } from '@/hooks/useShortcutScope'

const DashboardPage: React.FC = () => {
  const { t } = useTranslation()
  const { searchValue } = useSearch()
  const [currentFilter, setCurrentFilter] = useState<Filter>(Filter.All)
  const { status: lifecycleStatusDto, retry: retryLifecycle, retrying } = useLifecycleStatus()

  useShortcutScope('clipboard')
  const { hasMore, handleLoadMore } = useClipboardEvents(currentFilter)

  const handleFilterChange = (filterId: Filter) => {
    setCurrentFilter(filterId)
  }

  return (
    <div className="flex flex-col h-full relative">
      {/* Top search bar - Hidden in MVP */}
      <Header onFilterChange={handleFilterChange} className="hidden" />

      {/* Lifecycle failure banner */}
      {(lifecycleStatusDto?.state === 'WatcherFailed' ||
        lifecycleStatusDto?.state === 'NetworkFailed') && (
        <div className="mx-3 mt-2 mb-1 p-3 rounded-md bg-destructive/10 border border-destructive/20 flex items-center justify-between">
          <span className="text-sm font-medium text-destructive">
            {lifecycleStatusDto?.state === 'WatcherFailed'
              ? t('lifecycle.watcherFailed')
              : t('lifecycle.networkFailed')}
          </span>
          <button
            onClick={retryLifecycle}
            disabled={retrying}
            className="text-sm px-3 py-1 rounded bg-destructive/20 hover:bg-destructive/30 text-destructive font-medium disabled:opacity-50"
          >
            {retrying ? t('lifecycle.retrying') : t('lifecycle.retry')}
          </button>
        </div>
      )}

      {/* Clipboard content area - use flex-1 to make it take remaining space */}
      <div className="flex-1 overflow-hidden relative">
        <ClipboardContent
          filter={currentFilter}
          searchQuery={searchValue}
          hasMore={hasMore}
          onLoadMore={handleLoadMore}
        />
      </div>
    </div>
  )
}

export default DashboardPage
