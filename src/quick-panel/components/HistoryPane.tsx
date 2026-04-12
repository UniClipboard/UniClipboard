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
import { isMac, quickCardClassName } from '../constants'
import type { DisplayItem, TimeRangePreset } from '../types'
import PanelItem from './PanelItem'
import { Filter } from '@/api/clipboardItems'
import AdvancedSearch from '@/components/search/AdvancedSearch'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'

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
  onSelect: (index: number) => void
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
  }) => {
    const filterTypes = [
      { id: Filter.All, icon: Layers, label: 'All' },
      { id: Filter.Text, icon: FileText, label: 'Text' },
      { id: Filter.Image, icon: ImageIcon, label: 'Image' },
      { id: Filter.Link, icon: LinkIcon, label: 'Link' },
      { id: Filter.File, icon: Folder, label: 'File' },
      { id: Filter.Code, icon: Code, label: 'Code' },
    ]

    const timeRanges: { id: TimeRangePreset; label: string }[] = [
      { id: 'all_time', label: 'All time' },
      { id: 'today', label: 'Today' },
      { id: 'yesterday', label: 'Yesterday' },
      { id: 'last_7d', label: 'Last 7 days' },
      { id: 'last_30d', label: 'Last 30 days' },
      { id: 'this_week', label: 'This week' },
      { id: 'this_month', label: 'This month' },
    ]

    const CurrentFilterIcon = useMemo(() => {
      return filterTypes.find(f => f.id === activeFilter)?.icon || Search
    }, [activeFilter])

    return (
      <div className={quickCardClassName}>
        {isLocked && !loading ? (
          <>
            <div className="flex flex-1 flex-col items-center justify-center gap-4 px-6">
              <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-muted/30">
                <Lock className="h-6 w-6 text-muted-foreground" />
              </div>
              <div className="space-y-1 text-center">
                <h2 className="text-sm font-medium text-foreground">Clipboard is locked</h2>
                <p className="text-[12px] text-muted-foreground">
                  Unlock to access your clipboard history
                </p>
              </div>
              <button
                type="button"
                onClick={onUnlock}
                disabled={unlocking}
                className="flex items-center gap-1.5 rounded-md bg-primary px-4 py-1.5 text-[13px] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
              >
                {unlocking ? (
                  <>
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    Unlocking...
                  </>
                ) : (
                  <>
                    <Unlock className="h-3.5 w-3.5" />
                    Unlock
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
              <span>esc close</span>
            </div>
          </>
        ) : (
          <>
            {/* --- SPOTLIGHT STYLE TOP BAR --- */}
            <div className="border-b border-border/50 relative">
              <DropdownMenu>
                <AdvancedSearch
                  value={searchQuery}
                  onValueChange={onSearchChange}
                  isAdvanced={isAdvancedMode}
                  onAdvancedChange={setIsAdvancedMode}
                  tokens={tokens}
                  onTokensChange={setTokens}
                  inputRef={searchInputRef}
                  onKeyDown={onKeyDown}
                  icon={
                    <div
                      className={cn(
                        'flex items-center gap-0.5 px-1 py-0.5 rounded transition-all',
                        activeFilter !== Filter.All
                          ? 'text-primary bg-primary/5'
                          : 'text-muted-foreground/60'
                      )}
                    >
                      <CurrentFilterIcon className="h-3.5 w-3.5" />
                      {!isAdvancedMode && (
                        <ChevronDown className="h-2.5 w-2.5 opacity-40 shrink-0" />
                      )}
                    </div>
                  }
                  onIconClick={() => {}}
                  className="w-full"
                />
                <DropdownMenuTrigger asChild>
                  {/* Anchor trigger to the fixed width icon container */}
                  <button className="absolute left-3.5 top-2 w-8 h-8 z-10 opacity-0 cursor-pointer" />
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-36 ml-1">
                  {filterTypes.map(f => (
                    <DropdownMenuItem
                      key={f.id}
                      onClick={() => setActiveFilter(f.id)}
                      className="flex items-center gap-2 text-[12px]"
                    >
                      <f.icon className="h-3.5 w-3.5 opacity-70" />
                      {f.label}
                      {activeFilter === f.id && <Check className="ml-auto h-3 w-3 text-primary" />}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>

            {/* --- SCROLLABLE LIST --- */}
            <div
              className="scrollbar-thin flex-1 overflow-y-auto px-1.5 py-1"
              onMouseMove={() => {
                if (!hasPointerMovedSinceShow) onHistoryMouseMove()
                if (isKeyboardNav) setIsKeyboardNav(false)
              }}
              onMouseLeave={() => setHoveredIndex(null)}
            >
              {loading ? (
                <div className="flex h-full items-center justify-center text-[13px] text-muted-foreground">
                  <Loader2 className="h-4 w-4 animate-spin mr-2" />
                  Loading…
                </div>
              ) : isSearching && filteredItems.length === 0 ? (
                <div className="flex h-full items-center justify-center text-[13px] text-muted-foreground">
                  <Loader2 className="h-4 w-4 animate-spin mr-2" />
                  Searching…
                </div>
              ) : filteredItems.length === 0 ? (
                <div className="flex flex-col h-full items-center justify-center text-[13px] text-muted-foreground gap-2">
                  <div className="p-3 bg-muted/20 rounded-full">
                    {isAdvancedMode ? (
                      <Zap className="h-6 w-6 text-primary/40" />
                    ) : (
                      <Search className="h-6 w-6 text-muted-foreground/40" />
                    )}
                  </div>
                  <div className="text-center">
                    <p className="font-medium">No results found</p>
                    <p className="text-[11px] opacity-60">Try different keywords or filters</p>
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
            <div className="flex items-center justify-between gap-3 border-t border-border/50 px-4 py-1.5 text-[11px] text-muted-foreground bg-muted/5">
              <div className="flex items-center gap-3">
                <span className="shrink-0">{isMac ? '⌘' : '⌃'}1-0 paste</span>
                <div className="h-3 w-px bg-border/50" />

                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button className="flex items-center gap-1 hover:text-foreground transition-colors outline-none font-medium">
                      <span>{timeRanges.find(r => r.id === timeRange)?.label || 'All time'}</span>
                      <ChevronDown className="h-2.5 w-2.5 opacity-50" />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" side="top" className="w-36">
                    {timeRanges.map(range => (
                      <DropdownMenuItem
                        key={range.id}
                        onClick={() => setTimeRange(range.id)}
                        className="flex items-center justify-between text-[12px]"
                      >
                        {range.label}
                        {timeRange === range.id && <Check className="h-3 w-3 text-primary" />}
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>

              <div className="flex items-center gap-2">
                {(searchQuery || tokens.length > 0 || activeFilter !== Filter.All) && (
                  <span className="font-mono text-[10px] bg-muted/50 px-1.5 py-0.5 rounded leading-none">
                    {isSearching ? '…' : (searchTotal ?? filteredItems.length)} results
                  </span>
                )}
                <span className="truncate opacity-60">↑↓ navigate · ⏎ paste</span>
              </div>
            </div>
          </>
        )}
      </div>
    )
  }
)

export default HistoryPane
