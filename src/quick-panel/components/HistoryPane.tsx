import { Loader2, Lock, Search, Unlock } from 'lucide-react'
import React, { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import { CompositeSearchBar, type SourceOption } from '@/components/history/composite-search'
import type { SearchTagOption } from '@/lib/search-tags'
import { cn } from '@/lib/utils'
import { quickCardClassName } from '../constants'
import {
  peekQuickPanelImageAspectRatio,
  useQuickPanelImageAspectRatioEpoch,
} from '../hooks/useQuickPanelImage'
import { packImageWallColumns } from '../imageWallPacker'
import type { DisplayItem, TimeRangePreset } from '../types'
import ImageGridItem from './ImageGridItem'
import PanelItem from './PanelItem'

interface HistoryPaneProps {
  filteredItems: DisplayItem[]
  hasPointerMovedSinceShow: boolean
  isKeyboardNav: boolean
  isLocked: boolean
  isSearching: boolean
  searchTotal: number | null
  itemRefs: React.MutableRefObject<Map<number, HTMLDivElement>>
  loading: boolean
  onHover: (index: number) => void
  onHistoryMouseMove: () => void
  onSearchChange: (value: string) => void
  onSelect: (index: number, plainOnly?: boolean) => void
  onUnlock: () => void
  searchInputRef: React.RefObject<HTMLInputElement | null>
  searchQuery: string
  selectedIndex: number
  setHoveredIndex: React.Dispatch<React.SetStateAction<number | null>>
  setIsKeyboardNav: React.Dispatch<React.SetStateAction<boolean>>
  unlocking: boolean
  unlockError: string | null
  activeFilter: Filter
  setActiveFilter: (f: Filter) => void
  tagFilter: string | null
  setTagFilter: (tag: string | null) => void
  sourceFilter: string | null
  setSourceFilter: (source: string | null) => void
  extensionFilter: string | null
  setExtensionFilter: (extension: string | null) => void
  timeRange: TimeRangePreset
  setTimeRange: (t: TimeRangePreset) => void
  searchableTags: SearchTagOption[]
  sourceOptions: SourceOption[]
  onKeyDown: (e: React.KeyboardEvent<HTMLInputElement>) => void
}

/** How many columns the image-wall masonry uses. Small enough that individual
 *  tiles remain recognizable in the quick panel's constrained width. */
const IMAGE_WALL_COLUMN_COUNT = 3

/** Cmd/Ctrl + 1-9, 0 shortcut hint for a row's position in the list. */
function getShortcutKey(index: number): string | undefined {
  return index < 10 ? (index === 9 ? '0' : String(index + 1)) : undefined
}

/** Ref callback registering a row's DOM node under its list index, so
 * arrow-key navigation can `scrollIntoView` the selected row (see
 * `ClipboardHistoryPanel`'s `itemRefs`-keyed effect). */
function makeItemRef(
  itemRefs: React.MutableRefObject<Map<number, HTMLDivElement>>,
  index: number
): (el: HTMLDivElement | null) => void {
  return el => {
    if (el) itemRefs.current.set(index, el)
    else itemRefs.current.delete(index)
  }
}

const HistoryPane: React.FC<HistoryPaneProps> = React.memo(
  ({
    filteredItems,
    hasPointerMovedSinceShow,
    isKeyboardNav,
    isLocked,
    isSearching,
    searchTotal,
    itemRefs,
    loading,
    onHover,
    onHistoryMouseMove,
    onSearchChange,
    onSelect,
    onUnlock,
    searchInputRef,
    searchQuery,
    selectedIndex,
    setHoveredIndex,
    setIsKeyboardNav,
    unlocking,
    unlockError,
    activeFilter,
    setActiveFilter,
    tagFilter,
    setTagFilter,
    sourceFilter,
    setSourceFilter,
    extensionFilter,
    setExtensionFilter,
    timeRange,
    setTimeRange,
    searchableTags,
    sourceOptions,
    onKeyDown,
  }) => {
    const { t } = useTranslation(undefined, { keyPrefix: 'quickPanel.history' })
    const aspectRatioEpoch = useQuickPanelImageAspectRatioEpoch()

    const showImageWall = activeFilter === Filter.Image
    // Greedy column packer for the image wall: reads each tile's cached aspect
    // ratio (published by <img.onLoad> via reportQuickPanelImageAspectRatio) so
    // known entries pack correctly on the very first paint. New entries start
    // at a 1:1 assumption and repack once their ratio is measured — the epoch
    // dep below wakes this memo up whenever any tile publishes a new ratio.
    const imageColumns = useMemo(() => {
      if (!showImageWall) return null
      return packImageWallColumns(filteredItems, IMAGE_WALL_COLUMN_COUNT, item =>
        peekQuickPanelImageAspectRatio(item.id)
      )
    }, [filteredItems, showImageWall, aspectRatioEpoch])

    return (
      <div className={quickCardClassName}>
        {isLocked && !loading ? (
          <>
            <div className="flex flex-1 flex-col items-center justify-center gap-4 px-6">
              <div className="flex size-12 items-center justify-center rounded-xl bg-muted/30">
                <Lock className="size-6 text-muted-foreground" />
              </div>
              <div className="space-y-1 text-center">
                <h2 className="text-sm font-medium text-foreground">{t('locked.title')}</h2>
                <p className="text-[12px] text-muted-foreground">{t('locked.description')}</p>
              </div>
              <button
                type="button"
                onClick={onUnlock}
                disabled={unlocking}
                className="flex items-center gap-1.5 rounded-md bg-primary px-4 py-1.5 text-[13px] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
              >
                {unlocking ? (
                  <>
                    <Loader2 className="size-3.5 animate-spin" />
                    {t('locked.unlocking')}
                  </>
                ) : (
                  <>
                    <Unlock className="size-3.5" />
                    {t('locked.action')}
                  </>
                )}
              </button>
              {unlockError && (
                <p className="max-w-[15rem] text-center text-[12px] text-destructive">
                  {unlockError}
                </p>
              )}
            </div>
            <div className="flex items-center justify-center border-t border-border/50 px-3 py-1.5 text-[11px] text-muted-foreground">
              <span>{t('status.close')}</span>
            </div>
          </>
        ) : (
          <>
            {/* --- SPOTLIGHT STYLE TOP BAR --- */}
            <div className="border-b border-border/50 px-3 py-2">
              <CompositeSearchBar
                contentFilter={activeFilter}
                tagFilter={tagFilter}
                sourceFilter={sourceFilter}
                extensionFilter={extensionFilter}
                timeRange={timeRange}
                onContentFilterChange={setActiveFilter}
                onTagFilterChange={setTagFilter}
                onSourceFilterChange={setSourceFilter}
                onExtensionFilterChange={setExtensionFilter}
                onTimeRangeChange={setTimeRange}
                onQueryChange={onSearchChange}
                onQuerySubmit={text => onSearchChange(text.trim())}
                sourceOptions={sourceOptions}
                tagOptions={searchableTags}
                totalCount={searchTotal ?? filteredItems.length}
                inputRef={searchInputRef}
                onUnhandledKeyDown={onKeyDown}
                clearShortcutEnabled={false}
                suggestionActivation="intentional"
                showFilterPanelButton
                className="w-full"
              />
            </div>

            {/* --- SCROLLABLE LIST --- */}
            {/* role="listbox" 给下面 PanelItem 的 role="option" 提供合法父级。
                未接 aria-activedescendant 的完整 combobox 链路:焦点恒在搜索框,
                选中态由 isSelected/aria-selected 表达,够当前键盘导航用。 */}
            <div
              role="listbox"
              aria-label={t('listAriaLabel')}
              className={cn(
                'scrollbar-thin flex-1 overflow-y-auto overflow-x-hidden px-1.5 py-1',
                // Reserved scrollbar gutter so a mid-scroll appearance of the
                // scrollbar doesn't shove the masonry tiles sideways.
                showImageWall && 'overflow-y-scroll px-2 py-2 [scrollbar-gutter:stable]'
              )}
              onMouseMove={() => {
                if (!hasPointerMovedSinceShow) onHistoryMouseMove()
                if (isKeyboardNav) setIsKeyboardNav(false)
              }}
              onMouseLeave={() => setHoveredIndex(null)}
            >
              {loading ? (
                <div className="flex h-full items-center justify-center text-[13px] text-muted-foreground">
                  <Loader2 className="size-4 animate-spin mr-2" />
                  {t('status.loading')}
                </div>
              ) : isSearching && filteredItems.length === 0 ? (
                <div className="flex h-full items-center justify-center text-[13px] text-muted-foreground">
                  <Loader2 className="size-4 animate-spin mr-2" />
                  {t('status.searching')}
                </div>
              ) : filteredItems.length === 0 ? (
                <div className="flex flex-col h-full items-center justify-center text-[13px] text-muted-foreground gap-2">
                  <div className="p-3 bg-muted/20 rounded-full">
                    <Search className="size-6 text-muted-foreground/40" />
                  </div>
                  <div className="text-center">
                    <p className="font-medium">{t('empty.title')}</p>
                    <p className="text-[11px] opacity-60">{t('empty.description')}</p>
                  </div>
                </div>
              ) : showImageWall && imageColumns ? (
                <div className="flex w-full min-w-0 items-start gap-1">
                  {imageColumns.map((column, columnIndex) => (
                    <div key={columnIndex} className="flex min-w-0 flex-1 flex-col">
                      {column.map(({ item, index }) => (
                        <ImageGridItem
                          key={item.id}
                          item={item}
                          index={index}
                          isSelected={index === selectedIndex}
                          hoverDisabled={isKeyboardNav}
                          onSelect={onSelect}
                          onHover={onHover}
                          shortcutKey={getShortcutKey(index)}
                          itemRef={makeItemRef(itemRefs, index)}
                        />
                      ))}
                    </div>
                  ))}
                </div>
              ) : (
                filteredItems.map((item, index) => (
                  <PanelItem
                    key={item.id}
                    item={item}
                    index={index}
                    isSelected={index === selectedIndex}
                    hoverDisabled={isKeyboardNav}
                    onSelect={onSelect}
                    onHover={onHover}
                    shortcutKey={getShortcutKey(index)}
                    itemRef={makeItemRef(itemRefs, index)}
                  />
                ))
              )}
            </div>

            {/* --- MULTI-FUNCTION STATUS BAR --- */}
            <div className="flex items-center justify-between gap-3 border-t border-border/50 bg-muted/5 px-4 py-1.5 text-[11px] text-muted-foreground">
              <div className="flex items-center" />

              <div className="flex items-center gap-2">
                {(searchQuery ||
                  activeFilter !== Filter.All ||
                  tagFilter !== null ||
                  sourceFilter !== null ||
                  extensionFilter !== null ||
                  timeRange !== 'all_time') && (
                  <span className="font-mono text-[10px] bg-muted/50 px-1.5 py-0.5 rounded leading-none">
                    {isSearching ? '…' : (searchTotal ?? filteredItems.length)}
                  </span>
                )}
                <span className="truncate opacity-60">{t('status.navigatePaste')}</span>
              </div>
            </div>
          </>
        )}
      </div>
    )
  }
)

export default HistoryPane
