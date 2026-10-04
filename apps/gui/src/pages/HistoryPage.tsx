import { m } from 'framer-motion'
import React, { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import { countSearch } from '@/api/daemon/search'
import ClipboardActionBar from '@/components/clipboard/ClipboardActionBar'
import ClipboardPreview from '@/components/clipboard/ClipboardPreview'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import { HistoryFilterPanel } from '@/components/history/composite-search'
import { CompositeSearchBarView } from '@/components/history/composite-search/CompositeSearchBar'
import {
  type CompositeSearchBarProps,
  useCompositeSearchBar,
} from '@/components/history/composite-search/useCompositeSearchBar'
import { useZeroResultRelaxations } from '@/components/history/composite-search/useZeroResultRelaxations'
import ZeroResultRelaxations from '@/components/history/composite-search/ZeroResultRelaxations'
import {
  HISTORY_ENTRY_ANIMATION,
  HISTORY_PREVIEW_ENTRY_TRANSITION,
} from '@/components/history/history-entry-animation'
import HistoryGrid from '@/components/history/HistoryGrid'
import { HISTORY_LIBRARY_FILTER_STATE } from '@/components/history/sidebar/history-sidebar-types'
import HistorySidebar from '@/components/history/sidebar/HistorySidebar'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { useHistoryController } from '@/hooks/useHistoryController'
import { useShortcut } from '@/hooks/useShortcut'

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const c = useHistoryController()
  const searchProps: CompositeSearchBarProps = {
    contentFilter: c.filter.activeFilter,
    sourceFilter: c.filter.sourceFilter,
    tagFilter: c.filter.tagFilter,
    timeRange: c.filter.timeRange,
    extensionFilter: c.filter.extensionFilter,
    onContentFilterChange: c.filterActions.setContentFilter,
    onTagFilterChange: c.filterActions.setTagFilter,
    onSourceFilterChange: c.filterActions.setSourceFilter,
    onTimeRangeChange: c.filterActions.setTimeRange,
    onExtensionFilterChange: c.filterActions.setExtensionFilter,
    onQueryChange: c.filterActions.setQuery,
    onQuerySubmit: text => c.filterActions.submitQuery(text.trim()),
    sourceOptions: c.sourceOptions,
    tagOptions: c.searchableTags,
    totalCount: c.browseCount,
    inputRef: c.searchInputRef,
    fetchCounts: countSearch,
  }
  const compositeSearch = useCompositeSearchBar(searchProps)
  const relaxations = useZeroResultRelaxations({
    active: c.isSearchActive && !c.searchLoading && c.items.length === 0,
    chips: compositeSearch.chips,
    current: compositeSearch.current,
    query: c.filter.submittedQuery.trim(),
    fetchCounts: countSearch,
  })

  // A Library row picked on the Devices page arrives as router state; apply it
  // once, then drop it so back/forward navigation does not re-apply it.
  const location = useLocation()
  const navigate = useNavigate()
  const libraryFilter = (location.state as Record<string, unknown> | null)?.[
    HISTORY_LIBRARY_FILTER_STATE
  ] as Filter | undefined
  const { setContentFilter } = c.filterActions
  useEffect(() => {
    if (!libraryFilter) return
    setContentFilter(libraryFilter)
    navigate(location.pathname, { replace: true, state: null })
  }, [libraryFilter, location.pathname, navigate, setContentFilter])

  const focusSearch = () => c.searchInputRef.current?.focus()
  useShortcut({
    id: 'clipboard.search',
    key: 'mod+f',
    scope: 'clipboard',
    handler: focusSearch,
    enableOnFormTags: true,
  })
  useShortcut({
    key: ['/', '、'],
    scope: 'clipboard',
    handler: focusSearch,
    useKey: true,
  })

  return (
    <div className="relative flex h-full flex-col">
      {/* ── Degraded notice: index rebuilding, browse served from main store ─ */}
      {c.indexState === 'degraded' && (
        <div className="shrink-0 mx-2 mb-2 rounded-md bg-amber-500/10 px-3 py-1.5 text-ui-caption text-amber-600 dark:text-amber-400">
          {t('clipboard.search.degraded')}
        </div>
      )}

      {/* ── Library sidebar + list + preview ── */}
      <div className="flex min-h-0 flex-1">
        <HistorySidebar
          context="history"
          activeFilter={c.filter.activeFilter}
          onSelectLibrary={c.filterActions.setContentFilter}
        />
        <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
          {/* List */}
          <ResizablePanel id="history-list" defaultSize="42%" minSize="20rem" maxSize="36rem">
            <div className="flex h-full min-w-0 flex-col">
              <div className="flex shrink-0 flex-col gap-2 px-3 pb-2 pt-3">
                <CompositeSearchBarView {...searchProps} state={compositeSearch} />
                <HistoryFilterPanel
                  contentFilter={c.filter.activeFilter}
                  sourceFilter={c.filter.sourceFilter}
                  tagFilter={c.filter.tagFilter}
                  timeRange={c.filter.timeRange}
                  extensionFilter={c.filter.extensionFilter}
                  onContentFilterChange={c.filterActions.setContentFilter}
                  onTagFilterChange={c.filterActions.setTagFilter}
                  onSourceFilterChange={c.filterActions.setSourceFilter}
                  onTimeRangeChange={c.filterActions.setTimeRange}
                  onExtensionFilterChange={c.filterActions.setExtensionFilter}
                  sourceOptions={c.sourceOptions}
                  tagOptions={c.searchableTags}
                />
              </div>
              <HistoryGrid
                items={c.items}
                seenIds={c.seenIds}
                selectedId={c.selectedId}
                listRef={c.listRef}
                restoreStateFrom={c.scrollState}
                isSearchActive={c.isSearchActive}
                submittedQuery={c.filter.submittedQuery}
                searchLoading={c.searchLoading}
                copySuccessId={c.copySuccessId}
                deletingId={c.deletingId}
                hasMore={c.hasMore}
                onLoadMore={c.handleLoadMore}
                onCopy={c.handleCopy}
                onFilePathsAction={c.handleCopyFilePaths}
                onDelete={c.requestDelete}
                onToggleFavorite={c.handleToggleFavorite}
                onCardClick={c.handleCardClick}
                onHoverChange={c.handleHoverChange}
                onScrollStateRestored={() => c.setScrollState(null)}
                emptyStateActions={
                  relaxations && (
                    <ZeroResultRelaxations
                      relaxations={relaxations}
                      onRemove={compositeSearch.resetDimension}
                    />
                  )
                }
              />
            </div>
          </ResizablePanel>

          <ResizableHandle />

          {/* Preview */}
          <ResizablePanel id="history-preview" defaultSize="58%" minSize="35%">
            <m.div
              data-testid="history-preview-motion"
              initial={HISTORY_ENTRY_ANIMATION.initial}
              animate={HISTORY_ENTRY_ANIMATION.animate}
              transition={HISTORY_PREVIEW_ENTRY_TRANSITION}
              className="relative flex h-full min-w-0 flex-col"
            >
              <ClipboardPreview
                item={c.selectedItem}
                actions={delivery => (
                  <ClipboardActionBar
                    item={c.selectedItem}
                    delivery={delivery}
                    copySuccess={c.copySuccessId !== null && c.copySuccessId === c.selectedId}
                    onCopy={() => {
                      if (c.selectedId) c.handleCopy(c.selectedId)
                    }}
                    onToggleFavorite={() => {
                      if (c.selectedItem) {
                        c.handleToggleFavorite(
                          c.selectedItem.id,
                          c.selectedItem.isFavorited === true
                        )
                      }
                    }}
                    onDelete={() => {
                      if (c.selectedId) c.requestDelete(c.selectedId)
                    }}
                  />
                )}
              />
            </m.div>
          </ResizablePanel>
        </ResizablePanelGroup>
      </div>

      <DeleteConfirmDialog
        open={c.deleteDialogOpen}
        onOpenChange={c.setDeleteDialogOpen}
        onConfirm={c.confirmDelete}
        count={1}
      />
    </div>
  )
}

export default HistoryPage
