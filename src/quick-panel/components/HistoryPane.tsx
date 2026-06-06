import {
  ChevronDown,
  Layers,
  Code,
  FileText,
  Image as ImageIcon,
  Link as LinkIcon,
  Loader2,
  Lock,
  Search,
  Unlock,
  Check,
  Zap,
  Folder,
} from 'lucide-react'
import React, { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import AdvancedSearch from '@/components/search/AdvancedSearch'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { quickCardClassName, QUICK_FILTER_ORDER } from '../constants'
import type { DisplayItem, TimeRangePreset } from '../types'
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
  searchInputRef: React.Ref<HTMLInputElement>
  searchQuery: string
  selectedIndex: number
  setHoveredIndex: React.Dispatch<React.SetStateAction<number | null>>
  setIsKeyboardNav: React.Dispatch<React.SetStateAction<boolean>>
  unlocking: boolean
  unlockError: string | null
  activeFilter: Filter
  setActiveFilter: (f: Filter) => void
  timeRange: TimeRangePreset
  setTimeRange: (t: TimeRangePreset) => void
  isAdvancedMode: boolean
  setIsAdvancedMode: (v: boolean) => void
  tokens: string[]
  setTokens: (t: string[]) => void
  onKeyDown: (e: KeyboardEvent) => void
  focusSearchInput: () => void
}

// Icon per content-type filter. Keyed by Filter; QUICK_FILTER_ORDER decides
// which ones are shown and in what order.
const FILTER_ICONS: Partial<Record<Filter, React.ElementType>> = {
  [Filter.All]: Layers,
  [Filter.Text]: FileText,
  [Filter.Image]: ImageIcon,
  [Filter.Link]: LinkIcon,
  [Filter.File]: Folder,
  [Filter.Code]: Code,
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
    timeRange,
    setTimeRange,
    isAdvancedMode,
    setIsAdvancedMode,
    tokens,
    setTokens,
    onKeyDown,
    focusSearchInput,
  }) => {
    const { t } = useTranslation(undefined, { keyPrefix: 'quickPanel.history' })

    const filterTypes = useMemo(
      () =>
        QUICK_FILTER_ORDER.map(id => ({
          id,
          icon: FILTER_ICONS[id] ?? Search,
          label: t(`filters.${id}`),
        })),
      [t]
    )

    const timeRanges: { id: TimeRangePreset; label: string }[] = [
      { id: 'all_time', label: t('timeRange.all_time') },
      { id: 'today', label: t('timeRange.today') },
      { id: 'yesterday', label: t('timeRange.yesterday') },
      { id: 'last_7d', label: t('timeRange.last_7d') },
      { id: 'last_30d', label: t('timeRange.last_30d') },
      { id: 'this_week', label: t('timeRange.this_week') },
      { id: 'this_month', label: t('timeRange.this_month') },
    ]

    const CurrentFilterIcon = useMemo(() => {
      return filterTypes.find(f => f.id === activeFilter)?.icon || Search
    }, [activeFilter, filterTypes])

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
            <div className="border-b border-border/50">
              <AdvancedSearch
                value={searchQuery}
                onValueChange={onSearchChange}
                isAdvanced={isAdvancedMode}
                onAdvancedChange={setIsAdvancedMode}
                tokens={tokens}
                onTokensChange={setTokens}
                placeholder={t('searchPlaceholder')}
                advancedPlaceholder={t('advancedPlaceholder')}
                inputRef={searchInputRef}
                onKeyDown={onKeyDown}
                leftSlot={
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        aria-label={t('filterMenuLabel')}
                        className={cn(
                          'flex items-center gap-0.5 rounded px-1.5 py-1 outline-none transition-colors hover:bg-muted/50',
                          activeFilter !== Filter.All ? 'text-primary' : 'text-muted-foreground/50'
                        )}
                      >
                        <CurrentFilterIcon className="size-3.5" />
                        <ChevronDown className="size-2.5 shrink-0 opacity-50" />
                      </button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent
                      align="start"
                      className="w-36"
                      onCloseAutoFocus={e => {
                        // Keep focus on the search input so arrow keys keep
                        // driving the list instead of re-opening this menu.
                        e.preventDefault()
                        focusSearchInput()
                      }}
                    >
                      {filterTypes.map(f => (
                        <DropdownMenuItem
                          key={f.id}
                          onClick={() => setActiveFilter(f.id)}
                          className="flex items-center gap-2 text-[12px]"
                        >
                          <f.icon className="size-3.5 opacity-70" />
                          {f.label}
                          {activeFilter === f.id && (
                            <Check className="ml-auto size-3 text-primary" />
                          )}
                        </DropdownMenuItem>
                      ))}
                    </DropdownMenuContent>
                  </DropdownMenu>
                }
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
              className="scrollbar-thin flex-1 overflow-y-auto px-1.5 py-1"
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
                    {isAdvancedMode ? (
                      <Zap className="size-6 text-primary/40" />
                    ) : (
                      <Search className="size-6 text-muted-foreground/40" />
                    )}
                  </div>
                  <div className="text-center">
                    <p className="font-medium">{t('empty.title')}</p>
                    <p className="text-[11px] opacity-60">{t('empty.description')}</p>
                  </div>
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
                    shortcutKey={index < 10 ? (index === 9 ? '0' : String(index + 1)) : undefined}
                    itemRef={el => {
                      if (el) itemRefs.current.set(index, el)
                      else itemRefs.current.delete(index)
                    }}
                  />
                ))
              )}
            </div>

            {/* --- MULTI-FUNCTION STATUS BAR --- */}
            <div className="flex items-center justify-between gap-3 border-t border-border/50 bg-muted/5 px-4 py-1.5 text-[11px] text-muted-foreground">
              <div className="flex items-center">
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      className="flex items-center gap-1 hover:text-foreground transition-colors outline-none font-medium"
                    >
                      <span>
                        {timeRanges.find(r => r.id === timeRange)?.label || t('timeRange.all_time')}
                      </span>
                      <ChevronDown className="size-2.5 opacity-50" />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="start"
                    side="top"
                    className="w-36"
                    onCloseAutoFocus={e => {
                      e.preventDefault()
                      focusSearchInput()
                    }}
                  >
                    {timeRanges.map(range => (
                      <DropdownMenuItem
                        key={range.id}
                        onClick={() => setTimeRange(range.id)}
                        className="flex items-center justify-between text-[12px]"
                      >
                        {range.label}
                        {timeRange === range.id && <Check className="size-3 text-primary" />}
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>

              <div className="flex items-center gap-2">
                {(searchQuery || tokens.length > 0 || activeFilter !== Filter.All) && (
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
