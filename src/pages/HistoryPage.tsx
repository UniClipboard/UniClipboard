import React, { useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import ClipboardActionBar from '@/components/clipboard/ClipboardActionBar'
import ClipboardPreview from '@/components/clipboard/ClipboardPreview'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import { CompositeSearchBar, HistoryFilterPanel } from '@/components/history/composite-search'
import HistoryGrid from '@/components/history/HistoryGrid'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { useTitleBarSlot } from '@/contexts/titlebar-slot-context'
import { useHistoryController } from '@/hooks/useHistoryController'
import { usePlatform } from '@/hooks/usePlatform'

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const { isMac } = usePlatform()
  const { setRightSlot } = useTitleBarSlot()
  const c = useHistoryController()

  // The composite search box is shared between the in-page top bar (non-mac) and
  // the window title bar (mac). Memoized so its element reference only changes
  // when an input prop actually changes — required because injecting it into the
  // title bar slot re-renders the app root, which would otherwise loop.
  const searchBox = useMemo(
    () => (
      <CompositeSearchBar
        contentFilter={c.filter.activeFilter}
        sourceFilter={c.filter.sourceFilter}
        tagFilter={c.filter.tagFilter}
        timeRange={c.filter.timeRange}
        onContentFilterChange={c.filterActions.setContentFilter}
        onTagFilterChange={c.filterActions.setTagFilter}
        onSourceFilterChange={c.filterActions.setSourceFilter}
        onTimeRangeChange={c.filterActions.setTimeRange}
        onQueryChange={c.filterActions.setQuery}
        onQuerySubmit={text => c.filterActions.submitQuery(text.trim())}
        sourceOptions={c.sourceOptions}
        tagOptions={c.searchableTags}
        totalCount={c.browseCount}
        inputRef={c.searchInputRef}
      />
    ),
    [
      c.filter.activeFilter,
      c.filter.sourceFilter,
      c.filter.tagFilter,
      c.filter.timeRange,
      c.filterActions,
      c.sourceOptions,
      c.searchableTags,
      c.browseCount,
      c.searchInputRef,
    ]
  )

  // On mac, hoist just the search box into the otherwise-empty title bar drag
  // region (no heading); on other platforms it stays in the in-page top bar.
  const titleBarContent = useMemo(
    () => (isMac ? <div className="w-80 max-w-full">{searchBox}</div> : null),
    [isMac, searchBox]
  )

  useEffect(() => {
    if (!isMac) return
    setRightSlot(titleBarContent)
    return () => setRightSlot(null)
  }, [isMac, titleBarContent, setRightSlot])

  return (
    <div className="flex flex-col h-full">
      {/* ── Top bar: page heading (left) + composite search (right) ─ */}
      {/* On mac this whole row moves into the window title bar (see above). */}
      {!isMac && (
        <div className="shrink-0 flex items-center gap-3 border-b border-border/60 px-4 pt-3 pb-2.5">
          <h1 className="shrink-0 text-sm font-semibold text-foreground">{c.viewLabel}</h1>
          <div className="ml-auto w-80 max-w-full">{searchBox}</div>
        </div>
      )}

      {/* ── Degraded notice: index rebuilding, browse served from main store ─ */}
      {c.indexState === 'degraded' && (
        <div className="shrink-0 mx-2 mb-2 rounded-md bg-amber-500/10 px-3 py-1.5 text-xs text-amber-600 dark:text-amber-400">
          {t('clipboard.search.degraded')}
        </div>
      )}

      {/* ── Organize panel (left) + list/preview master-detail (right) ── */}
      <div className="flex min-h-0 flex-1">
        <HistoryFilterPanel
          contentFilter={c.filter.activeFilter}
          sourceFilter={c.filter.sourceFilter}
          tagFilter={c.filter.tagFilter}
          timeRange={c.filter.timeRange}
          onContentFilterChange={c.filterActions.setContentFilter}
          onTagFilterChange={c.filterActions.setTagFilter}
          onSourceFilterChange={c.filterActions.setSourceFilter}
          onTimeRangeChange={c.filterActions.setTimeRange}
          sourceOptions={c.sourceOptions}
          tagOptions={c.searchableTags}
        />
        <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
          {/* List */}
          <ResizablePanel id="history-list" defaultSize="42%" minSize="30%" maxSize="65%">
            <div className="flex h-full min-w-0 flex-col">
              <HistoryGrid
                items={c.items}
                seenIds={c.seenIds}
                selectedId={c.selectedId}
                listRef={c.listRef}
                restoreStateFrom={c.scrollState}
                isSearchActive={c.isSearchActive}
                submittedQuery={c.filter.submittedQuery}
                searchLoading={c.searchLoading}
                hoveredId={c.hoveredId}
                copySuccessId={c.copySuccessId}
                deletingId={c.deletingId}
                hasMore={c.hasMore}
                onLoadMore={c.handleLoadMore}
                onCopy={c.handleCopy}
                onDelete={c.requestDelete}
                onToggleFavorite={c.handleToggleFavorite}
                onCardClick={c.handleCardClick}
                onHoverChange={c.setHoveredId}
                onScrollStateRestored={() => c.setScrollState(null)}
              />
            </div>
          </ResizablePanel>

          <ResizableHandle />

          {/* Preview */}
          <ResizablePanel id="history-preview" defaultSize="58%" minSize="35%">
            <div className="flex h-full min-w-0 flex-col">
              <ClipboardPreview
                item={c.selectedItem}
                actions={
                  <ClipboardActionBar
                    hasActiveItem={c.selectedItem !== null}
                    copySuccess={c.copySuccessId !== null && c.copySuccessId === c.selectedId}
                    onCopy={() => {
                      if (c.selectedId) c.handleCopy(c.selectedId)
                    }}
                    onDelete={() => {
                      if (c.selectedId) c.requestDelete(c.selectedId)
                    }}
                  />
                }
              />
            </div>
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
