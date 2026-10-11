import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { countSearch } from '@/api/daemon/search'
import { useCompositeSearchBar } from '@/components/history/composite-search/useCompositeSearchBar'
import { useZeroResultRelaxations } from '@/components/history/composite-search/useZeroResultRelaxations'
import { LIBRARY_BUILTIN_TAG_IDS } from '@/components/history/tags/history-tag-library'
import { NO_TAG_COLORS, TagColorsContext } from '@/components/history/tags/tag-colors-context'
import { useLibraryChrome } from '@/contexts/library-chrome-context'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { useHistoryController } from '@/hooks/useHistoryController'
import { useShortcut } from '@/hooks/useShortcut'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { Pane, SplitView, Text, UiRoot, type MenuEntry } from '@/ui-kit'
import { selectionSummary } from './history-kit-model'
import { HistoryKitDeleteAlert } from './HistoryKitDeleteAlert'
import { HistoryKitDetail } from './HistoryKitDetail'
import { HistoryKitFilters } from './HistoryKitFilters'
import { HistoryKitFirstLaunch } from './HistoryKitFirstLaunch'
import { HistoryKitGrid } from './HistoryKitGrid'
import { HistoryKitList } from './HistoryKitList'
import { HistoryKitNoResults } from './HistoryKitNoResults'
import { HistoryKitQueryFailed } from './HistoryKitQueryFailed'
import { HistoryKitScopeBar } from './HistoryKitScopeBar'
import { HistoryKitSearchField } from './HistoryKitSearchField'
import { HistoryKitSelection } from './HistoryKitSelection'
import { HistoryKitSidebar } from './HistoryKitSidebar'
import { HistoryKitTagManager } from './HistoryKitTagManager'
import { HistoryKitToolbar, HistoryKitViewSwitch } from './HistoryKitToolbar'

const VIEW_KEY = 'uc.history.kit.view'
const SIZE_KEY = 'uc.history.kit.gridSize'

function readPreference(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function writePreference(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // A preference that cannot be stored just does not survive a restart.
  }
}

const COPY = 'copy'
const PIN = 'pin'
const TAG = 'tag'
const DELETE = 'delete'

/**
 * The History window built on `@/ui-kit` (design canvas page "macOS - new — History"). All
 * business state and actions come from `useHistoryController`, the same owner as the existing
 * page; this component only composes the kit's regions. Which page a build uses is decided at
 * build time (`@ui-history-page` in vite.config.ts), never here.
 */
export default function HistoryKitPage() {
  const { t } = useTranslation()
  const c = useHistoryController()
  const chrome = useLibraryChrome()
  const colorScheme = useAppliedColorScheme()

  const search = useCompositeSearchBar({
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
  })
  const noResults = c.isSearchActive && !c.searchLoading && !c.queryFailure && c.items.length === 0
  const emptyHistory =
    !c.isSearchActive && !c.searchLoading && !c.queryFailure && c.items.length === 0
  const relaxations = useZeroResultRelaxations({
    active: noResults,
    chips: search.chips,
    current: search.current,
    query: c.filter.submittedQuery.trim(),
    fetchCounts: countSearch,
  })
  const terms = c.filter.submittedQuery.trim().split(/\s+/).filter(Boolean)

  // Quick Look steps through the loaded image entries around the selected one.
  const images = c.items.filter(item => item.type === 'image')
  const imageIndex = images.findIndex(item => item.id === c.selectedId)
  const imageNeighbors = {
    position: imageIndex + 1,
    total: images.length,
    previous: imageIndex > 0 ? () => c.handleCardClick(images[imageIndex - 1].id) : undefined,
    next:
      imageIndex >= 0 && imageIndex < images.length - 1
        ? () => c.handleCardClick(images[imageIndex + 1].id)
        : undefined,
  }

  // List or grid (design ME2) and the grid's card size are per-viewer conveniences.
  const [view, setView] = useState<'list' | 'grid'>(() =>
    readPreference(VIEW_KEY) === 'grid' ? 'grid' : 'list'
  )
  const [gridSize, setGridSize] = useState(() => {
    const stored = Number(readPreference(SIZE_KEY))
    return Number.isFinite(stored) && stored >= 0 && stored <= 1 ? stored : 0.4
  })
  // In the grid the detail is an inspector: double click or the shortcut opens it.
  const [inspector, setInspector] = useState(false)
  const changeView = (next: 'list' | 'grid') => {
    setView(next)
    writePreference(VIEW_KEY, next)
  }
  const changeGridSize = (next: number) => {
    setGridSize(next)
    writePreference(SIZE_KEY, String(next))
  }
  const showDetail = view === 'list' || inspector

  const multi = c.checkedItems.length > 1
  const selection = multi ? c.checkedItems.map(item => item.id) : c.selectedId ? [c.selectedId] : []
  const selectedItems = multi ? c.checkedItems : c.selectedItem ? [c.selectedItem] : []
  const allPinned = selectedItems.length > 0 && selectedItems.every(item => item.isFavorited)

  const select = (ids: string[]) => {
    const last = ids[ids.length - 1]
    if (last) c.handleCardClick(last)
    c.setChecked(ids.length > 1 ? ids : [])
  }
  const togglePin = () => {
    if (multi) c.pinChecked()
    else if (c.selectedItem) {
      void c.handleToggleFavorite(c.selectedItem.id, c.selectedItem.isFavorited === true)
    }
  }
  const deleteSelection = () => {
    if (selection.length > 0) c.requestDelete(selection)
  }
  const openTagManager = c.historyTags.available ? () => c.setTagManagerOpen(true) : undefined

  useShortcut({
    id: 'clipboard.search',
    key: 'mod+f',
    scope: 'clipboard',
    handler: () => c.searchInputRef.current?.focus(),
    enableOnFormTags: true,
  })
  useShortcut({
    key: 'alt+mod+i',
    scope: 'clipboard',
    enabled: view === 'grid',
    handler: () => setInspector(open => !open),
  })
  useShortcut({
    key: 'mod+p',
    scope: 'clipboard',
    enabled: selection.length > 0,
    handler: togglePin,
  })

  const contextMenu = {
    items: (ids: string[]): MenuEntry[] => {
      if (ids.length === 0) return []
      const items = c.items.filter(item => ids.includes(item.id))
      const pinned = items.every(item => item.isFavorited)
      return [
        ...(ids.length === 1
          ? [{ id: COPY, label: t('clipboard.contextMenu.copy'), icon: 'copy' as const }]
          : []),
        {
          id: PIN,
          label: t(pinned ? 'history.detail.unpin' : 'history.detail.pin'),
          icon: 'pin',
          shortcut: formatShortcutHint('mod+p'),
        },
        ...(c.historyTags.available
          ? [{ id: TAG, label: t('history.tags.tagEllipsis'), icon: 'tag' as const, shortcut: 'T' }]
          : []),
        { id: 'separator', kind: 'separator' },
        {
          id: DELETE,
          label: t('clipboard.contextMenu.delete'),
          icon: 'delete',
          destructive: true,
        },
      ]
    },
    onAction: (action: string, ids: string[]) => {
      if (action === COPY) void c.handleCopy(ids[0])
      else if (action === PIN) {
        const items = c.items.filter(item => ids.includes(item.id))
        const pin = items.some(item => item.isFavorited !== true)
        for (const item of items) {
          if ((item.isFavorited === true) !== pin) {
            void c.handleToggleFavorite(item.id, item.isFavorited === true)
          }
        }
      } else if (action === TAG) {
        if (ids.length > 1) c.setSelectionTagEditorOpen(true)
        else c.setTagEditorFor(ids[0])
      } else if (action === DELETE) c.requestDelete(ids)
    },
  }

  const selected = c.selectedItem
  const tagging =
    c.historyTags.available && selected
      ? {
          tags: c.historyTags.tags,
          editorOpen: c.tagEditorFor === selected.id,
          onEditorOpenChange: (open: boolean) => c.setTagEditorFor(open ? selected.id : null),
          onAdd: (tagId: string) => c.addTagToItems(tagId, [selected.id]),
          onCreate: (name: string, color: string) => c.tagItemsByName(name, [selected.id], color),
          onRemove: (tagId: string) => c.removeTagFromItems(tagId, [selected.id]),
        }
      : null

  return (
    <TagColorsContext value={c.tagLayout?.colors ?? NO_TAG_COLORS}>
      <UiRoot colorScheme={colorScheme}>
        <SplitView
          sidebar={
            <HistoryKitSidebar
              activeFilter={c.filter.activeFilter}
              activeTag={c.filter.tagFilter}
              onSelectLibrary={c.filterActions.setContentFilter}
              onSelectTag={c.filterActions.setTagFilter}
              tags={c.searchableTags}
              sidebarTagIds={c.tagLayout?.sidebar ?? null}
              tagColors={c.tagLayout?.colors ?? NO_TAG_COLORS}
              onManageTags={openTagManager}
              tagTotal={c.historyTags.tags.length + LIBRARY_BUILTIN_TAG_IDS.length}
              countsRevision={c.items}
            />
          }
          sidebarHidden={chrome.hidden && !chrome.drawerOpen}
          sidebarToggle={{
            label: t(chrome.hidden ? 'history.sidebar.show' : 'history.sidebar.hide'),
            onClick: chrome.toggle,
            shortcutHint: formatShortcutHint('ctrl+meta+s'),
          }}
          title={c.viewLabel}
          // A failed query has no count: showing 0 would claim an empty history.
          subtitle={
            c.queryFailure
              ? undefined
              : multi
                ? t('history.kit.selectedOf', {
                    count: c.checkedItems.length,
                    total: c.browseCount,
                  })
                : c.browseCount.toLocaleString()
          }
          toolbarLeading={
            <HistoryKitViewSwitch
              view={view}
              onViewChange={changeView}
              gridSize={gridSize}
              onGridSizeChange={changeGridSize}
            />
          }
          search={<HistoryKitSearchField search={search} inputRef={c.searchInputRef} />}
          toolbar={
            <HistoryKitToolbar
              targetIds={selection}
              pinned={allPinned}
              onTogglePin={togglePin}
              onDelete={deleteSelection}
              onManageTags={openTagManager}
            />
          }
        >
          <Pane kind={showDetail ? 'list' : 'main'} label={t('history.kit.listLabel')}>
            {c.isSearchActive ? (
              <HistoryKitScopeBar
                contentFilter={c.filter.activeFilter}
                onContentFilterChange={c.filterActions.setContentFilter}
                results={c.queryFailure ? null : c.browseCount}
                loading={c.searchLoading}
                onSeedOperator={search.seedDimension}
              />
            ) : (
              <HistoryKitFilters
                contentFilter={c.filter.activeFilter}
                sourceFilter={c.filter.sourceFilter}
                tagFilter={c.filter.tagFilter}
                timeRange={c.filter.timeRange}
                sourceOptions={c.sourceOptions}
                tags={c.searchableTags}
                onContentFilterChange={c.filterActions.setContentFilter}
                onSourceFilterChange={c.filterActions.setSourceFilter}
                onTagFilterChange={c.filterActions.setTagFilter}
                onTimeRangeChange={c.filterActions.setTimeRange}
              />
            )}
            {c.indexState === 'degraded' && (
              <div className="px-4 pb-1">
                <Text role="caption" tone="secondary">
                  {t('clipboard.search.degraded')}
                </Text>
              </div>
            )}
            {c.queryFailure ? (
              <HistoryKitQueryFailed reason={c.queryFailure} onRetry={c.retryQuery} />
            ) : emptyHistory ? (
              <HistoryKitFirstLaunch />
            ) : noResults ? (
              <HistoryKitNoResults
                filterCount={search.chips.length}
                relaxations={relaxations}
                onRemove={search.resetDimension}
              />
            ) : view === 'grid' ? (
              <HistoryKitGrid
                items={c.items}
                sourceDeviceNames={c.sourceDeviceNames}
                size={gridSize}
                selection={selection}
                onSelectionChange={select}
                onOpen={id => {
                  select([id])
                  setInspector(true)
                }}
                onDelete={ids => c.requestDelete(ids)}
                hasMore={c.hasMore}
                onLoadMore={c.handleLoadMore}
                contextMenu={contextMenu}
              />
            ) : (
              <HistoryKitList
                items={c.items}
                sourceDeviceNames={c.sourceDeviceNames}
                tagNames={c.historyTags.available ? c.tagNames : undefined}
                selection={selection}
                onSelectionChange={select}
                onCopy={id => void c.handleCopy(id)}
                onDelete={ids => c.requestDelete(ids)}
                hasMore={c.hasMore}
                loading={c.searchLoading}
                onLoadMore={c.handleLoadMore}
                contextMenu={contextMenu}
                highlight={terms}
              />
            )}
          </Pane>
          {showDetail && (
            <Pane kind="detail" label={t('history.detail.aria')}>
              {multi ? (
                <HistoryKitSelection
                  ids={selection}
                  summary={selectionSummary(c.checkedItems, t)}
                  tagging={
                    c.historyTags.available
                      ? {
                          tags: c.historyTags.tags,
                          summary: c.selectionTagSummary,
                          editorOpen: c.selectionTagEditorOpen,
                          onEditorOpenChange: c.setSelectionTagEditorOpen,
                          onAddToAll: tagId => c.addTagToItems(tagId, selection),
                          onCreateForAll: (name, color) => c.tagItemsByName(name, selection, color),
                          onRemoveFromAll: tagId => c.removeTagFromItems(tagId, selection),
                        }
                      : null
                  }
                  allPinned={allPinned}
                  onPinAll={c.pinChecked}
                  onDeleteAll={c.deleteChecked}
                  onDeselect={c.clearChecked}
                />
              ) : (
                <HistoryKitDetail
                  item={c.selectedItem}
                  emptyHistory={emptyHistory}
                  imageNeighbors={imageNeighbors}
                  tagging={tagging}
                  copySuccess={c.copySuccessId !== null && c.copySuccessId === c.selectedId}
                  onCopy={() => c.selectedId && void c.handleCopy(c.selectedId)}
                  onTogglePin={togglePin}
                  onDelete={deleteSelection}
                />
              )}
            </Pane>
          )}
        </SplitView>
        <HistoryKitTagManager
          open={c.tagManagerOpen && c.historyTags.available}
          onOpenChange={c.setTagManagerOpen}
          tags={c.historyTags.tags}
          searchTags={c.searchableTags}
          sidebarTagIds={c.tagLayout?.sidebar ?? null}
          tagColors={c.tagLayout?.colors ?? NO_TAG_COLORS}
          onCreate={c.createTag}
          onSetColor={c.setTagColor}
          onSetInSidebar={c.setTagInSidebar}
          onRename={c.renameTag}
          onMerge={c.mergeTagsInto}
          onDelete={c.deleteTags}
          onShowItems={tagId => {
            c.filterActions.setTagFilter(tagId)
            c.setTagManagerOpen(false)
          }}
        />
        <HistoryKitDeleteAlert
          open={c.deleteDialogOpen}
          onOpenChange={c.setDeleteDialogOpen}
          count={Math.max(c.deleteCount, 1)}
          onConfirm={c.confirmDelete}
        />
      </UiRoot>
    </TagColorsContext>
  )
}
