import { m } from 'framer-motion'
import React, { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import { countSearch } from '@/api/daemon/search'
import ClipboardActionBar from '@/components/clipboard/ClipboardActionBar'
import ClipboardPreview from '@/components/clipboard/ClipboardPreview'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import {
  HistoryFilterPanel,
  HistoryMorphingSearch,
  HistorySearchPanel,
} from '@/components/history/composite-search'
import { CompositeSearchBarView } from '@/components/history/composite-search/CompositeSearchBar'
import SearchFacetRow from '@/components/history/composite-search/SearchFacetRow'
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
import { useSidebarSlot } from '@/contexts/sidebar-slot-context'
import { useHistoryController } from '@/hooks/useHistoryController'
import { useShortcut } from '@/hooks/useShortcut'

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const c = useHistoryController()
  // The layout decides where search lives: a toolbar overlay where it offers a
  // toolbar host (Windows, Linux), the top of the list column where it does not
  // (macOS). Both read the same search state.
  const { contentToolbarHost } = useSidebarSlot()
  const searchProps: CompositeSearchBarProps = {
    variant: contentToolbarHost ? 'compact' : 'list',
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

  const [searchOpen, setSearchOpen] = useState(false)
  const searchControlRef = useRef<HTMLDivElement>(null)
  const searchSuggestionsOpen = compositeSearch.expanded && compositeSearch.buffer.trim().length > 0
  const hasActiveSearch =
    c.filter.submittedQuery.trim().length > 0 ||
    c.filter.activeFilter !== 'all' ||
    c.filter.tagFilter !== null ||
    c.filter.sourceFilter !== null ||
    c.filter.timeRange !== 'all_time' ||
    c.filter.extensionFilter !== null

  useEffect(() => {
    if (!searchOpen) return

    const frame = requestAnimationFrame(() => c.searchInputRef.current?.focus())
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node
      if (searchControlRef.current?.contains(target)) return
      setSearchOpen(false)
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setSearchOpen(false)
    }

    document.addEventListener('pointerdown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      cancelAnimationFrame(frame)
      document.removeEventListener('pointerdown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [searchOpen, c.searchInputRef])

  const focusSearch = () =>
    contentToolbarHost ? setSearchOpen(true) : c.searchInputRef.current?.focus()
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

  const filterPanel = (
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
  )

  return (
    <div className="relative flex h-full flex-col">
      {contentToolbarHost
        ? createPortal(
            <div className="flex items-center gap-2">
              {filterPanel}
              <HistoryMorphingSearch
                open={searchOpen}
                active={hasActiveSearch}
                containerRef={searchControlRef}
                inputRef={c.searchInputRef}
                value={compositeSearch.buffer}
                suggestionsOpen={searchSuggestionsOpen}
                suggestionsId={compositeSearch.panelId}
                title={t('history.composite.title')}
                placeholder={t('history.searchPlaceholder')}
                resultsLabel={t('history.composite.results', { count: c.browseCount })}
                clearAllLabel={t('history.composite.clearAll')}
                onInputChange={compositeSearch.handleInputChange}
                onInputKeyDown={compositeSearch.handleKeyDown}
                onClearAll={() => compositeSearch.clearAll()}
                onOpenChange={setSearchOpen}
              >
                <HistorySearchPanel
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
                  searchPanelId={compositeSearch.panelId}
                  searchOptions={compositeSearch.options}
                  searchHighlight={compositeSearch.clampedHighlight}
                  searchSuggestionsOpen={searchSuggestionsOpen}
                  onSearchOptionSelect={compositeSearch.selectOption}
                  onSearchOptionHighlight={compositeSearch.setHighlight}
                  onDismissSearchSuggestions={() => compositeSearch.setOpen(false)}
                />
              </HistoryMorphingSearch>
            </div>,
            contentToolbarHost
          )
        : null}
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
          <ResizablePanel
            id="history-list"
            // The macOS three-column design gives the list 560px (HList.dc.html).
            defaultSize={contentToolbarHost ? '42%' : '35rem'}
            minSize="20rem"
            maxSize="36rem"
          >
            <div className="flex h-full min-w-0 flex-col">
              {!contentToolbarHost && (
                // HList.dc.html: query bar, facet row, summary.
                <div className="shrink-0">
                  <div className="px-4 py-2.5">
                    <CompositeSearchBarView
                      {...searchProps}
                      shortcutHint="⌘F"
                      state={compositeSearch}
                    />
                  </div>
                  <div className="flex h-11 items-center border-b border-border/60 px-4">
                    <SearchFacetRow
                      chips={compositeSearch.chips}
                      onSeedDimension={compositeSearch.seedDimension}
                      onClearAll={() => compositeSearch.clearAll()}
                    />
                  </div>
                  <div className="flex h-10 items-center border-b border-border/40 px-4.5 text-ui-caption">
                    <span className="font-semibold text-foreground">
                      {t('history.subtitle', { count: c.browseCount })}
                    </span>
                  </div>
                </div>
              )}
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
                      variant={searchProps.variant}
                    />
                  )
                }
                emptyStateText={
                  relaxations && searchProps.variant === 'list'
                    ? {
                        title: t('history.composite.relaxTitleAll', {
                          count: compositeSearch.chips.length,
                        }),
                        subtitle: t('history.composite.relaxPrompt'),
                      }
                    : undefined
                }
              />
            </div>
          </ResizablePanel>

          <ResizableHandle />

          {/* Preview */}
          <ResizablePanel
            id="history-preview"
            defaultSize={contentToolbarHost ? '58%' : undefined}
            minSize="35%"
          >
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
