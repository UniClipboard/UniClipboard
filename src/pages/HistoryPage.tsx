import { m } from 'framer-motion'
import { Search, X } from 'lucide-react'
import React, { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import ClipboardActionBar from '@/components/clipboard/ClipboardActionBar'
import ClipboardPreview from '@/components/clipboard/ClipboardPreview'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import { HistoryFilterPanel, HistorySearchPanel } from '@/components/history/composite-search'
import { useCompositeSearchBar } from '@/components/history/composite-search/useCompositeSearchBar'
import {
  HISTORY_ENTRY_ANIMATION,
  HISTORY_PREVIEW_ENTRY_TRANSITION,
} from '@/components/history/history-entry-animation'
import HistoryGrid from '@/components/history/HistoryGrid'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { useSidebarSlot } from '@/contexts/sidebar-slot-context'
import { useHistoryController } from '@/hooks/useHistoryController'
import { useShortcut } from '@/hooks/useShortcut'

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const { contentToolbarHost } = useSidebarSlot()
  const c = useHistoryController()
  const [searchOpen, setSearchOpen] = useState(false)
  const searchControlRef = useRef<HTMLDivElement>(null)
  const searchPanelRef = useRef<HTMLDivElement>(null)
  const compositeSearch = useCompositeSearchBar({
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
  })
  const searchSuggestionsOpen = compositeSearch.expanded && compositeSearch.buffer.trim().length > 0

  useShortcut({
    id: 'clipboard.search',
    key: 'mod+f',
    scope: 'clipboard',
    handler: () => setSearchOpen(true),
    enableOnFormTags: true,
  })

  useShortcut({
    key: ['/', '、'],
    scope: 'clipboard',
    handler: () => setSearchOpen(true),
    useKey: true,
  })

  const searchBox = useMemo(
    () => (
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
        totalCount={c.browseCount}
        searchOptions={compositeSearch.options}
        searchHighlight={compositeSearch.clampedHighlight}
        searchSuggestionsOpen={searchSuggestionsOpen}
        onSearchOptionSelect={compositeSearch.selectOption}
        onSearchOptionHighlight={compositeSearch.setHighlight}
        onDismissSearchSuggestions={() => compositeSearch.setOpen(false)}
        onClearAll={() => compositeSearch.clearAll()}
        onClose={() => setSearchOpen(false)}
      />
    ),
    [
      c.filter.activeFilter,
      c.filter.sourceFilter,
      c.filter.tagFilter,
      c.filter.timeRange,
      c.filter.extensionFilter,
      c.filterActions,
      c.sourceOptions,
      c.searchableTags,
      c.browseCount,
      compositeSearch.options,
      compositeSearch.clampedHighlight,
      compositeSearch.selectOption,
      compositeSearch.setHighlight,
      compositeSearch.setOpen,
      compositeSearch.clearAll,
      searchSuggestionsOpen,
    ]
  )

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
      if (searchPanelRef.current?.contains(target) || searchControlRef.current?.contains(target))
        return
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

  const searchControl = useMemo(
    () => (
      <m.div
        data-tauri-drag-region="false"
        ref={searchControlRef}
        className={`flex shrink-0 items-center overflow-hidden transition-[background-color,border-radius,box-shadow] ${
          searchOpen ? 'bg-muted/50 ring-1 ring-primary/40' : 'bg-transparent ring-0'
        } ${searchOpen ? 'h-7 w-48 rounded-full' : 'size-8 rounded-md'}`}
      >
        {searchOpen ? (
          <div className="flex min-w-48 items-center gap-2 px-2.5">
            <Search className="size-3.5 shrink-0 text-muted-foreground" />
            <input
              ref={c.searchInputRef}
              value={compositeSearch.buffer}
              onChange={compositeSearch.handleInputChange}
              onKeyDown={compositeSearch.handleKeyDown}
              aria-label={t('history.searchPlaceholder')}
              autoCorrect="off"
              autoCapitalize="off"
              autoComplete="off"
              spellCheck={false}
              placeholder={t('history.searchPlaceholder')}
              className="min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground"
            />
            <button
              type="button"
              aria-label={t('history.composite.close')}
              onClick={() => setSearchOpen(false)}
              className="flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-background hover:text-foreground"
            >
              <X className="size-3.5" />
            </button>
          </div>
        ) : (
          <button
            type="button"
            aria-label={t('history.composite.title')}
            aria-expanded="false"
            onClick={() => setSearchOpen(true)}
            className="relative flex size-8 shrink-0 items-center justify-center text-muted-foreground hover:text-foreground"
          >
            <Search className="size-4" />
            {hasActiveSearch && (
              <span
                aria-hidden
                className="absolute right-1 top-1 size-1.5 rounded-full bg-primary"
              />
            )}
          </button>
        )}
      </m.div>
    ),
    [
      c.searchInputRef,
      compositeSearch.buffer,
      compositeSearch.handleInputChange,
      compositeSearch.handleKeyDown,
      hasActiveSearch,
      searchOpen,
      t,
    ]
  )

  return (
    <div className="relative flex h-full flex-col">
      {contentToolbarHost
        ? createPortal(
            <div className="flex items-center gap-2">
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
              {searchControl}
            </div>,
            contentToolbarHost
          )
        : null}
      {/* ── Degraded notice: index rebuilding, browse served from main store ─ */}
      {c.indexState === 'degraded' && (
        <div className="shrink-0 mx-2 mb-2 rounded-md bg-amber-500/10 px-3 py-1.5 text-xs text-amber-600 dark:text-amber-400">
          {t('clipboard.search.degraded')}
        </div>
      )}

      {/* ── List + preview master-detail ── */}
      <div className="flex min-h-0 flex-1">
        <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
          {/* List */}
          <ResizablePanel id="history-list" defaultSize="42%" minSize="20rem" maxSize="36rem">
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
                onFilePathsAction={c.handleCopyFilePaths}
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
            <m.div
              data-testid="history-preview-motion"
              initial={HISTORY_ENTRY_ANIMATION.initial}
              animate={HISTORY_ENTRY_ANIMATION.animate}
              transition={HISTORY_PREVIEW_ENTRY_TRANSITION}
              className="relative flex h-full min-w-0 flex-col"
            >
              {searchOpen && (
                <m.div
                  ref={searchPanelRef}
                  initial={{ opacity: 0, y: -8 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ duration: 0.16, ease: [0.22, 1, 0.36, 1] }}
                  className="absolute right-3 top-3 z-50 w-96 max-w-[calc(100%-1.5rem)]"
                >
                  {searchBox}
                </m.div>
              )}
              <ClipboardPreview
                item={c.selectedItem}
                actions={
                  <ClipboardActionBar
                    hasActiveItem={c.selectedItem !== null}
                    copySuccess={c.copySuccessId !== null && c.copySuccessId === c.selectedId}
                    isFavorited={c.selectedItem?.isFavorited === true}
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
                }
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
