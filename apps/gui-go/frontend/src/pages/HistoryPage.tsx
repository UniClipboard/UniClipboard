import { m } from 'framer-motion'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate } from 'react-router'
import { Filter } from '@/api/clipboardItems'
import { countSearch } from '@/api/daemon/search'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import { CompositeSearchBarView } from '@/components/history/composite-search/CompositeSearchBar'
import SearchFacetRow from '@/components/history/composite-search/SearchFacetRow'
import {
  type CompositeSearchBarProps,
  useCompositeSearchBar,
} from '@/components/history/composite-search/useCompositeSearchBar'
import { useZeroResultRelaxations } from '@/components/history/composite-search/useZeroResultRelaxations'
import ZeroResultRelaxations from '@/components/history/composite-search/ZeroResultRelaxations'
import HistoryDetailPanel from '@/components/history/detail/HistoryDetailPanel'
import type { DetailTagsProps } from '@/components/history/detail/HistoryDetailTags'
import HistorySelectionPanel, {
  type SelectionTaggingProps,
} from '@/components/history/detail/HistorySelectionPanel'
import {
  HISTORY_ENTRY_ANIMATION,
  HISTORY_PREVIEW_ENTRY_TRANSITION,
} from '@/components/history/history-entry-animation'
import HistoryGrid from '@/components/history/HistoryGrid'
import { DETAIL_COLUMN_MIN } from '@/components/history/layout/history-layout'
import { useHistoryListColumn } from '@/components/history/layout/useHistoryListColumn'
import HistoryBulkBar from '@/components/history/list/HistoryBulkBar'
import {
  HISTORY_LIBRARY_FILTER_STATE,
  HISTORY_TAG_FILTER_STATE,
} from '@/components/history/sidebar/history-sidebar-types'
import HistorySidebar from '@/components/history/sidebar/HistorySidebar'
import TrafficLightOverhang from '@/components/history/sidebar/TrafficLightOverhang'
import { LIBRARY_BUILTIN_TAG_IDS } from '@/components/history/tags/history-tag-library'
import HistoryTagManager from '@/components/history/tags/HistoryTagManager'
import { NO_TAG_COLORS, TagColorsContext } from '@/components/history/tags/tag-colors-context'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { useHistoryController } from '@/hooks/useHistoryController'
import { usePlatform } from '@/hooks/usePlatform'
import { useShortcut } from '@/hooks/useShortcut'

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const c = useHistoryController()
  const { isMac } = usePlatform()
  // The sidebar control that opened the tag Library by keyboard, if any.
  const [tagManagerReturnFocus, setTagManagerReturnFocus] = useState<HTMLElement | null>(null)
  const searchProps: CompositeSearchBarProps = {
    variant: 'list',
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
    onCreateTag: c.historyTags.available ? c.createTagForSearch : undefined,
  }
  const compositeSearch = useCompositeSearchBar(searchProps)
  const listColumn = useHistoryListColumn()
  const relaxations = useZeroResultRelaxations({
    active: c.isSearchActive && !c.searchLoading && c.items.length === 0,
    chips: compositeSearch.chips,
    current: compositeSearch.current,
    query: c.filter.submittedQuery.trim(),
    fetchCounts: countSearch,
  })

  // A Library or Tags row picked on the Devices page arrives as router state;
  // apply it once, then drop it so back/forward navigation does not re-apply it.
  const location = useLocation()
  const navigate = useNavigate()
  const routerState = location.state as Record<string, unknown> | null
  const libraryFilter = routerState?.[HISTORY_LIBRARY_FILTER_STATE] as Filter | undefined
  const tagFilter = routerState?.[HISTORY_TAG_FILTER_STATE] as string | undefined
  const { setContentFilter, setTagFilter } = c.filterActions
  useEffect(() => {
    if (!libraryFilter && !tagFilter) return
    if (libraryFilter) setContentFilter(libraryFilter)
    if (tagFilter) setTagFilter(tagFilter)
    navigate(location.pathname, { replace: true, state: null })
  }, [libraryFilter, tagFilter, location.pathname, navigate, setContentFilter, setTagFilter])

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

  const copySelected = () => {
    if (c.selectedId) c.handleCopy(c.selectedId)
  }
  const toggleSelectedFavorite = () => {
    if (c.selectedItem) {
      c.handleToggleFavorite(c.selectedItem.id, c.selectedItem.isFavorited === true)
    }
  }
  const deleteSelected = () => {
    if (c.selectedId) c.requestDelete(c.selectedId)
  }
  const selectedCopySuccess = c.copySuccessId !== null && c.copySuccessId === c.selectedId
  // Local tags of the selected entry for the detail column; hidden while
  // tags are unavailable.
  const detailTagging = (): DetailTagsProps | null => {
    const selected = c.selectedItem
    return c.historyTags.available && selected
      ? {
          tags: c.historyTags.tags,
          editorOpen: c.tagEditorFor === selected.id,
          onEditorOpenChange: open => c.setTagEditorFor(open ? selected.id : null),
          onAdd: tagId => c.addTagToItems(tagId, [selected.id]),
          onCreate: (name, color) => c.tagItemsByName(name, [selected.id], color),
          onRemove: tagId => c.removeTagFromItems(tagId, [selected.id]),
        }
      : null
  }
  // The checked rows' tags, shown in place of the detail while two or more are checked.
  const selectionTagging = (): SelectionTaggingProps | null => {
    if (!c.historyTags.available) return null
    const ids = c.checkedItems.map(item => item.id)
    return {
      tags: c.historyTags.tags,
      summary: c.selectionTagSummary,
      editorOpen: c.selectionTagEditorOpen,
      onEditorOpenChange: c.setSelectionTagEditorOpen,
      onAddToAll: tagId => c.addTagToItems(tagId, ids),
      onCreateForAll: (name, color) => c.tagItemsByName(name, ids, color),
      onRemoveFromAll: tagId => c.removeTagFromItems(tagId, ids),
    }
  }

  return (
    <TagColorsContext value={c.tagLayout?.colors ?? NO_TAG_COLORS}>
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
            tags={c.searchableTags}
            sidebarTagIds={c.tagLayout?.sidebar ?? null}
            activeTag={c.filter.tagFilter}
            onSelectTag={setTagFilter}
            tagLibrary={
              c.historyTags.available
                ? {
                    total: c.historyTags.tags.length + LIBRARY_BUILTIN_TAG_IDS.length,
                    open: returnFocusTo => {
                      setTagManagerReturnFocus(returnFocusTo)
                      c.setTagManagerOpen(true)
                    },
                  }
                : undefined
            }
            countsRevision={c.items}
          />
          <ResizablePanelGroup
            orientation="horizontal"
            className="min-h-0 flex-1"
            {...listColumn.groupProps}
          >
            {/* List */}
            <ResizablePanel
              id="history-list"
              // Per-window-tier constraints (history-layout.ts); the list keeps
              // its pixel width while the detail column flexes.
              {...listColumn.panelProps}
            >
              <div className="relative flex h-full min-w-0 flex-col">
                {/* HList.dc.html: query bar and facet row. */}
                <div className="shrink-0">
                  <div className="flex py-2.5 pl-4 pr-4">
                    <TrafficLightOverhang className="w-3.5 self-stretch" />
                    <div className="min-w-0 flex-1">
                      <CompositeSearchBarView
                        {...searchProps}
                        shortcutHint={isMac ? '⌘F' : 'Ctrl+F'}
                        state={compositeSearch}
                      />
                    </div>
                  </div>
                  <div className="flex items-center overflow-x-auto px-4 pb-1.75 pt-0.5 [scrollbar-width:none]">
                    <SearchFacetRow
                      chips={compositeSearch.chips}
                      onSeedDimension={compositeSearch.seedDimension}
                      onClearAll={() => compositeSearch.clearAll()}
                    />
                  </div>
                </div>
                <HistoryGrid
                  items={c.items}
                  sourceDeviceNames={c.sourceDeviceNames}
                  tagNames={c.historyTags.available ? c.tagNames : undefined}
                  seenIds={c.seenIds}
                  selectedId={c.selectedId}
                  listRef={c.listRef}
                  restoreStateFrom={c.scrollState}
                  isSearchActive={c.isSearchActive}
                  submittedQuery={c.filter.submittedQuery}
                  searchLoading={c.searchLoading}
                  copySuccessId={c.copySuccessId}
                  deletingIds={c.deletingIds}
                  checkedIds={c.checkedIds}
                  onToggleChecked={c.toggleChecked}
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
                {c.checkedItems.length > 0 && (
                  <HistoryBulkBar
                    items={c.checkedItems}
                    onPin={c.pinChecked}
                    onTag={
                      c.historyTags.available && c.checkedItems.length > 1
                        ? () => c.setSelectionTagEditorOpen(true)
                        : undefined
                    }
                    onDelete={c.deleteChecked}
                  />
                )}
              </div>
            </ResizablePanel>

            <ResizableHandle {...listColumn.handleProps} />

            {/* Preview */}
            <ResizablePanel id="history-preview" minSize={`${DETAIL_COLUMN_MIN}px`}>
              <m.div
                data-testid="history-preview-motion"
                initial={HISTORY_ENTRY_ANIMATION.initial}
                animate={HISTORY_ENTRY_ANIMATION.animate}
                transition={HISTORY_PREVIEW_ENTRY_TRANSITION}
                className="relative flex h-full min-w-0 flex-col"
              >
                {/* The selection's tags while 2+ rows are checked, else the detail column. */}
                {c.checkedItems.length > 1 && c.historyTags.available ? (
                  <HistorySelectionPanel
                    count={c.checkedItems.length}
                    tagging={selectionTagging()}
                  />
                ) : (
                  <HistoryDetailPanel
                    item={c.selectedItem}
                    tagging={detailTagging()}
                    copySuccess={selectedCopySuccess}
                    onCopy={copySelected}
                    onToggleFavorite={toggleSelectedFavorite}
                    onDelete={deleteSelected}
                  />
                )}
              </m.div>
            </ResizablePanel>
          </ResizablePanelGroup>
        </div>

        <HistoryTagManager
          open={c.tagManagerOpen && c.historyTags.available}
          returnFocusTo={tagManagerReturnFocus}
          onOpenChange={c.setTagManagerOpen}
          tags={c.historyTags.tags}
          searchTags={c.searchableTags}
          sidebarTagIds={c.tagLayout?.sidebar ?? null}
          onCreate={c.createTag}
          onSetColor={c.setTagColor}
          onSetInSidebar={c.setTagInSidebar}
          onRename={c.renameTag}
          onMerge={c.mergeTagsInto}
          onDelete={c.deleteTags}
          onShowItems={tagId => {
            setTagFilter(tagId)
            c.setTagManagerOpen(false)
          }}
        />

        <DeleteConfirmDialog
          open={c.deleteDialogOpen}
          onOpenChange={c.setDeleteDialogOpen}
          onConfirm={c.confirmDelete}
          count={Math.max(c.deleteCount, 1)}
        />
      </div>
    </TagColorsContext>
  )
}

export default HistoryPage
