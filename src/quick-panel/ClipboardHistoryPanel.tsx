import { listen } from '@tauri-apps/api/event'
import React, { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import { Filter } from '@/api/clipboardItems'
import { deleteClipboardEntry, restoreClipboardEntry } from '@/api/daemon'
import { unlockEncryptionSession } from '@/api/security'
import { useClipboardCollection } from '@/hooks/useClipboardCollection'
import { useDebounce } from '@/hooks/useDebounce'
import { useThemeSync } from '@/hooks/useThemeSync'
import { getItemPreview, resolveItemType } from '@/lib/clipboard-utils'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { readStoredUiScale, subscribeUiScaleChanges } from '@/lib/ui-scale'
import { cn } from '@/lib/utils'
import ClipboardPreviewPane from './ClipboardPreviewPane'
import HistoryPane from './components/HistoryPane'
import { PREVIEW_OPEN_DELAY_MS, PREVIEW_SWITCH_DELAY_MS, QUICK_FILTER_ORDER } from './constants'
import { useHistorySearch } from './hooks/useHistorySearch'
import type { DisplayItem, PreviewAction, PreviewState, TimeRangePreset } from './types'

const log = createLogger('clipboard-history-panel')

async function dismissPanel(): Promise<void> {
  await commands.dismissQuickPanel()
}

async function pasteToApp(): Promise<void> {
  await commands.pasteToPreviousApp()
}

async function setQuickPanelLayout(scale: number, previewExpanded: boolean): Promise<void> {
  await commands.setQuickPanelLayout(scale, previewExpanded)
}

/** Which side the inline preview opens toward, relative to the history pane. */
type PreviewSide = 'left' | 'right'

/**
 * Ask the backend which side the preview will open toward, before the window
 * moves. Used to reverse the flex layout ahead of the reposition so the pinned
 * history pane doesn't visibly jump when the preview opens leftward. Defaults
 * to 'right' on any failure.
 */
async function resolveExpandSide(scale: number): Promise<PreviewSide> {
  try {
    const side = await commands.resolveQuickPanelExpandSide(scale)
    return side === 'left' ? 'left' : 'right'
  } catch {
    return 'right'
  }
}

const initialPreviewState: PreviewState = {
  entryId: null,
  mode: 'closed',
  suppressed: false,
  historyLockedWidth: null,
  focusSource: 'selection',
}

function previewReducer(state: PreviewState, action: PreviewAction): PreviewState {
  switch (action.type) {
    case 'reset':
      return { ...initialPreviewState, suppressed: action.suppressed ?? false }
    case 'suppress':
      return state.suppressed === action.value ? state : { ...state, suppressed: action.value }
    case 'set-entry':
      return state.entryId === action.entryId ? state : { ...state, entryId: action.entryId }
    case 'set-focus-source':
      return state.focusSource === action.source ? state : { ...state, focusSource: action.source }
    case 'reserve-space':
      return {
        entryId: action.entryId,
        mode: 'reserving',
        suppressed: false,
        historyLockedWidth: action.historyLockedWidth,
        focusSource: state.focusSource,
      }
    case 'expand':
      if (state.mode === 'expanded') return state
      return { ...state, mode: 'expanded', historyLockedWidth: null }
    default:
      return state
  }
}

const ClipboardHistoryPanel: React.FC = () => {
  useThemeSync()

  const { items, loading, isLocked, reload } = useClipboardCollection()
  const [searchQuery, setSearchQuery] = useState('')
  const debouncedSearchQuery = useDebounce(searchQuery, 300)
  const [activeFilter, setActiveFilter] = useState<Filter>(Filter.All)
  const [timeRange, setTimeRange] = useState<TimeRangePreset>('all_time')
  const [isAdvancedMode, setIsAdvancedMode] = useState(false)
  const [tokens, setTokens] = useState<string[]>([])

  const [selectedIndex, setSelectedIndex] = useState(0)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const [isKeyboardNav, setIsKeyboardNav] = useState(true)
  const [unlocking, setUnlocking] = useState(false)
  const [unlockError, setUnlockError] = useState<string | null>(null)
  const [uiScale, setUiScale] = useState(() => readStoredUiScale())
  const [previewState, dispatchPreview] = useReducer(previewReducer, initialPreviewState)
  const [hasPointerMovedSinceShow, setHasPointerMovedSinceShow] = useState(false)
  const [previewTargetId, setPreviewTargetId] = useState<string | null>(null)

  const searchInputRef = useRef<HTMLInputElement>(null)
  const historyPaneRef = useRef<HTMLDivElement>(null)
  const itemRefs = useRef<Map<number, HTMLDivElement>>(new Map())
  const previewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const previewLayoutTokenRef = useRef(0)
  const deletingRef = useRef(false)
  const [skipTransition, setSkipTransition] = useState(false)
  // Which side the preview opens toward. Driven by the backend (it knows the
  // window's screen position): 'left' when the right edge can't fit the
  // expanded panel. Reversing the flex order keeps the history pane pinned.
  const [previewSide, setPreviewSide] = useState<PreviewSide>('right')

  const previewExpanded = previewState.mode === 'expanded'
  const previewReservingSpace = previewState.mode === 'reserving'
  const previewEntryId = previewState.entryId
  const previewSuppressed = previewState.suppressed
  const historyLockedWidth = previewState.historyLockedWidth
  const previewFocusSource = previewState.focusSource

  const displayItems = useMemo<DisplayItem[]>(
    () =>
      items.map(item => ({
        id: item.id,
        type: resolveItemType(item),
        preview: getItemPreview(item),
        activeTime: item.active_time,
        isUnavailable: item.payload_state === 'Lost',
      })),
    [items]
  )

  const { filteredItems, isSearching, searchTotal } = useHistorySearch({
    items: displayItems,
    searchQuery: debouncedSearchQuery,
    tokens,
    activeFilter,
    timeRange,
    isAdvancedMode,
  })

  const clearPreviewTimer = useCallback(() => {
    if (previewTimerRef.current) {
      clearTimeout(previewTimerRef.current)
      previewTimerRef.current = null
    }
  }, [])

  const closePreview = useCallback(
    (suppressUntilNextSelection: boolean) => {
      clearPreviewTimer()
      previewLayoutTokenRef.current += 1
      dispatchPreview({ type: 'reset', suppressed: suppressUntilNextSelection })
      setPreviewSide('right')
      void setQuickPanelLayout(readStoredUiScale(), false).catch(() => {})
    },
    [clearPreviewTimer]
  )

  useEffect(() => {
    let finalizeTimer: ReturnType<typeof setTimeout> | null = null
    const unlistenPrepare = listen('quick-panel://prepare-show', () => {
      setSkipTransition(true)
      clearPreviewTimer()
      previewLayoutTokenRef.current += 1
      dispatchPreview({ type: 'reset' })
      setPreviewSide('right')
      setSearchQuery('')
      setTokens([])
      setIsAdvancedMode(false)
      setActiveFilter(Filter.All)
      setTimeRange('all_time')
      setSelectedIndex(0)
      setHoveredIndex(null)
      setIsKeyboardNav(true)
      setHasPointerMovedSinceShow(false)
      setPreviewTargetId(null)
      void reload()

      finalizeTimer = setTimeout(() => {
        finalizeTimer = null
        setSkipTransition(false)
        void setQuickPanelLayout(readStoredUiScale(), false)
          .then(() => commands.finalizeQuickPanelShow())
          .then(() => searchInputRef.current?.focus())
          .catch(() => {})
      }, 0)
    })

    return () => {
      clearPreviewTimer()
      if (finalizeTimer !== null) {
        clearTimeout(finalizeTimer)
        finalizeTimer = null
      }
      unlistenPrepare.then(fn => fn())
    }
  }, [clearPreviewTimer, reload])

  useEffect(() => {
    void setQuickPanelLayout(uiScale, previewExpanded).catch(() => {})
  }, [previewExpanded, uiScale])

  useEffect(() => subscribeUiScaleChanges(setUiScale), [])

  useEffect(() => {
    const handleWindowKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      setHoveredIndex(null)
      void dismissPanel()
    }

    window.addEventListener('keydown', handleWindowKeyDown)
    return () => window.removeEventListener('keydown', handleWindowKeyDown)
  }, [])

  const handleUnlock = useCallback(async () => {
    setUnlocking(true)
    setUnlockError(null)
    try {
      await unlockEncryptionSession()
      setUnlocking(false)
      void reload()
    } catch (err) {
      setUnlocking(false)
      setUnlockError(err instanceof Error ? err.message : String(err))
    }
  }, [reload])

  useEffect(() => {
    if (isLocked) closePreview(false)
    else {
      setUnlocking(false)
      setUnlockError(null)
    }
  }, [closePreview, isLocked])

  // Reset selectedIndex to 0 whenever the visible result set shape changes
  // (filter/search/etc). Done inline during render with a tracking ref to
  // avoid a useEffect that chains a state update behind another state.
  // Deletes set deletingRef.current=true so we keep the user's current row.
  // The ref imperatively detects "filteredItems length changed" during the
  // same render — intentional, see https://react.dev/learn/you-might-not-need-an-effect#adjusting-some-state-when-a-prop-changes
  /* eslint-disable react-hooks/refs */
  const lastFilteredCountRef = useRef(filteredItems.length)
  if (lastFilteredCountRef.current !== filteredItems.length) {
    lastFilteredCountRef.current = filteredItems.length
    if (deletingRef.current) {
      deletingRef.current = false
    } else if (selectedIndex !== 0) {
      setSelectedIndex(0)
    }
  }
  /* eslint-enable react-hooks/refs */

  useEffect(() => {
    const el = itemRefs.current.get(selectedIndex)
    el?.scrollIntoView?.({ block: 'nearest' })
  }, [selectedIndex])

  const selectedItem = filteredItems[selectedIndex] ?? null
  const hoveredItem = hoveredIndex != null ? (filteredItems[hoveredIndex] ?? null) : null
  const targetPreviewItem =
    previewTargetId != null
      ? (filteredItems.find(item => item.id === previewTargetId) ?? null)
      : null

  useEffect(() => {
    if (previewFocusSource === 'selection') setPreviewTargetId(selectedItem?.id ?? null)
  }, [previewFocusSource, selectedItem])

  useEffect(() => {
    if (previewFocusSource === 'hover' && hoveredItem) setPreviewTargetId(hoveredItem.id)
  }, [hoveredIndex, hoveredItem, previewFocusSource])

  useEffect(() => {
    clearPreviewTimer()
    if (previewSuppressed || isLocked) return
    if (!targetPreviewItem) {
      previewLayoutTokenRef.current += 1
      dispatchPreview({ type: 'reset' })
      setPreviewSide('right')
      void setQuickPanelLayout(uiScale, false).catch(() => {})
      return
    }
    if (previewEntryId === targetPreviewItem.id) return

    previewTimerRef.current = setTimeout(
      () => {
        const nextEntryId = targetPreviewItem.id
        dispatchPreview({ type: 'set-entry', entryId: nextEntryId })
        if (previewExpanded) return
        const token = previewLayoutTokenRef.current + 1
        const nextHistoryWidth = historyPaneRef.current?.getBoundingClientRect().width ?? 0
        previewLayoutTokenRef.current = token
        void (async () => {
          // Resolve the open side and reverse the layout BEFORE moving the
          // window, so the pinned history pane never jumps when opening left.
          //
          // The await stays ABOVE the token guard on purpose. `token` equals
          // the ref here (set just above), so the guard can only diverge WHILE
          // we're awaiting — it's a post-await staleness/cancellation check,
          // not a skippable early return. react-doctor's async-defer-await
          // wants the await moved below the guard, but that would drop the
          // cancellation window and apply a stale side. Keep this order.
          const side = await resolveExpandSide(uiScale)
          if (previewLayoutTokenRef.current !== token) return
          setPreviewSide(side)
          dispatchPreview({
            type: 'reserve-space',
            entryId: nextEntryId,
            historyLockedWidth: nextHistoryWidth > 0 ? nextHistoryWidth : null,
          })
          try {
            await setQuickPanelLayout(uiScale, true)
            if (previewLayoutTokenRef.current === token) dispatchPreview({ type: 'expand' })
          } catch {
            if (previewLayoutTokenRef.current === token) dispatchPreview({ type: 'reset' })
          }
        })()
      },
      previewEntryId ? PREVIEW_SWITCH_DELAY_MS : PREVIEW_OPEN_DELAY_MS
    )
    return () => {
      if (previewTimerRef.current) {
        clearTimeout(previewTimerRef.current)
        previewTimerRef.current = null
      }
    }
  }, [
    clearPreviewTimer,
    isLocked,
    previewEntryId,
    previewExpanded,
    previewSuppressed,
    targetPreviewItem,
    uiScale,
  ])

  const handleSelect = useCallback(
    async (index: number, plainOnly?: boolean) => {
      const item = filteredItems[index]
      if (!item) return
      try {
        await restoreClipboardEntry(item.id, plainOnly ? { plainOnly: true } : undefined)
        await pasteToApp()
      } catch (err) {
        log.error({ err }, 'Failed to restore clipboard entry')
      }
    },
    [filteredItems]
  )

  const handleHover = useCallback(
    (index: number) => {
      if (isKeyboardNav || !hasPointerMovedSinceShow) return
      const item = filteredItems[index]
      if (!item) return
      dispatchPreview({ type: 'suppress', value: false })
      dispatchPreview({ type: 'set-focus-source', source: 'hover' })
      setPreviewTargetId(item.id)
      setHoveredIndex(index)
    },
    [filteredItems, hasPointerMovedSinceShow, isKeyboardNav]
  )

  const handleDelete = useCallback(
    async (index: number) => {
      const item = filteredItems[index]
      if (!item) return
      try {
        await deleteClipboardEntry(item.id)
        deletingRef.current = true
        clearPreviewTimer()
        setHoveredIndex(null)
        dispatchPreview({ type: 'suppress', value: false })
        dispatchPreview({ type: 'set-focus-source', source: 'selection' })
        const remainingItems = filteredItems.filter((_, i) => i !== index)
        const nextIndex = remainingItems.length > 0 ? Math.min(index, remainingItems.length - 1) : 0
        setSelectedIndex(nextIndex)
        setPreviewTargetId(remainingItems[nextIndex]?.id ?? null)
        void reload()
      } catch (err) {
        log.error({ err }, 'Failed to delete clipboard entry')
      }
    },
    [clearPreviewTimer, filteredItems, reload]
  )

  const handleSearchChange = useCallback((value: string) => {
    dispatchPreview({ type: 'suppress', value: false })
    dispatchPreview({ type: 'set-focus-source', source: 'selection' })
    setHoveredIndex(null)
    setSearchQuery(value)
  }, [])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (isLocked) {
        if (e.key === 'Enter' && !unlocking) {
          e.preventDefault()
          void handleUnlock()
        }
        return
      }

      if (e.altKey && e.key === 'Backspace') {
        e.preventDefault()
        void handleDelete(selectedIndex)
        return
      }

      if ((e.metaKey || e.ctrlKey) && e.key >= '0' && e.key <= '9') {
        e.preventDefault()
        const index = e.key === '0' ? 9 : parseInt(e.key) - 1
        if (index < filteredItems.length) void handleSelect(index, e.altKey)
        return
      }

      if (e.ctrlKey && (e.key === 'n' || e.key === 'p')) {
        e.preventDefault()
        dispatchPreview({ type: 'suppress', value: false })
        dispatchPreview({ type: 'set-focus-source', source: 'selection' })
        setIsKeyboardNav(true)
        setHoveredIndex(null)
        setSelectedIndex(prev =>
          e.key === 'n' ? Math.min(prev + 1, filteredItems.length - 1) : Math.max(prev - 1, 0)
        )
        return
      }

      // Tab / Shift+Tab cycle the content-type filter without moving focus off
      // the search input (AdvancedSearch prevents the default tab + forwards the
      // event here). Order mirrors the filter dropdown via QUICK_FILTER_ORDER.
      if (e.key === 'Tab') {
        e.preventDefault()
        dispatchPreview({ type: 'suppress', value: false })
        dispatchPreview({ type: 'set-focus-source', source: 'selection' })
        setHoveredIndex(null)
        setActiveFilter(prev => {
          const count = QUICK_FILTER_ORDER.length
          // A filter outside the cycle (e.g. Favorited) maps to All as the base.
          const base = Math.max(0, QUICK_FILTER_ORDER.indexOf(prev))
          const next = e.shiftKey ? (base - 1 + count) % count : (base + 1) % count
          return QUICK_FILTER_ORDER[next]
        })
        return
      }

      switch (e.key) {
        case 'ArrowDown':
        case 'ArrowUp':
          e.preventDefault()
          dispatchPreview({ type: 'suppress', value: false })
          dispatchPreview({ type: 'set-focus-source', source: 'selection' })
          setIsKeyboardNav(true)
          setHoveredIndex(null)
          setSelectedIndex(prev =>
            e.key === 'ArrowDown'
              ? Math.min(prev + 1, filteredItems.length - 1)
              : Math.max(prev - 1, 0)
          )
          break
        case 'Enter':
          e.preventDefault()
          void handleSelect(selectedIndex, e.altKey)
          break
      }
    },
    [
      filteredItems.length,
      handleDelete,
      handleSelect,
      handleUnlock,
      isLocked,
      selectedIndex,
      unlocking,
    ]
  )

  const handleHistoryMouseMove = useCallback(() => setHasPointerMovedSinceShow(true), [])

  // Keyboard navigation (arrows/Enter) is bound to the search input only, so it
  // relies on the input keeping focus. The filter/time-range dropdowns steal
  // focus to their trigger button on close — pulling it back here keeps arrow
  // keys driving the list instead of re-opening the menu.
  const focusSearchInput = useCallback(() => {
    searchInputRef.current?.focus()
  }, [])

  return (
    <div
      className={cn(
        'flex h-screen w-screen overflow-hidden bg-transparent p-4',
        // Open the preview to the left of the history pane near the right edge.
        // Reversing the row keeps the history pane (now the last child) pinned
        // at its anchor while the preview occupies the freed space on the left.
        previewSide === 'left' && 'flex-row-reverse'
      )}
    >
      <div
        ref={historyPaneRef}
        className={
          previewReservingSpace && historyLockedWidth != null
            ? 'min-w-0 shrink-0'
            : 'min-w-0 flex-1 basis-0'
        }
        style={
          previewReservingSpace && historyLockedWidth != null
            ? { width: `${historyLockedWidth}px` }
            : undefined
        }
      >
        <HistoryPane
          filteredItems={filteredItems}
          hasPointerMovedSinceShow={hasPointerMovedSinceShow}
          isKeyboardNav={isKeyboardNav}
          isLocked={isLocked}
          isSearching={isSearching}
          searchTotal={searchTotal}
          itemRefs={itemRefs}
          loading={loading}
          onHover={handleHover}
          onHistoryMouseMove={handleHistoryMouseMove}
          onSearchChange={handleSearchChange}
          onSelect={handleSelect}
          onUnlock={handleUnlock}
          searchInputRef={searchInputRef}
          searchQuery={searchQuery}
          selectedIndex={selectedIndex}
          setHoveredIndex={setHoveredIndex}
          setIsKeyboardNav={setIsKeyboardNav}
          unlocking={unlocking}
          unlockError={unlockError}
          activeFilter={activeFilter}
          setActiveFilter={setActiveFilter}
          timeRange={timeRange}
          setTimeRange={setTimeRange}
          isAdvancedMode={isAdvancedMode}
          setIsAdvancedMode={setIsAdvancedMode}
          tokens={tokens}
          setTokens={setTokens}
          onKeyDown={handleKeyDown}
          focusSearchInput={focusSearchInput}
        />
      </div>

      <div
        className={cn(
          'min-w-0',
          // Gap between preview and history. With `flex-row-reverse` (preview
          // on the left) the gap must sit on the preview's right edge instead.
          previewExpanded
            ? cn(
                previewSide === 'left' ? 'mr-2' : 'ml-2',
                'flex-1 basis-0 opacity-100 translate-x-0'
              )
            : previewReservingSpace && historyLockedWidth != null
              ? cn(
                  previewSide === 'left' ? 'mr-2' : 'ml-2',
                  'shrink-0 opacity-0 translate-x-0 pointer-events-none'
                )
              : 'ml-0 w-0 opacity-0 translate-x-2 pointer-events-none'
        )}
        style={
          previewReservingSpace && historyLockedWidth != null
            ? { width: `max(0px, calc(100% - ${historyLockedWidth}px - 0.5rem))` }
            : undefined
        }
        aria-hidden={!previewExpanded}
      >
        <div
          className={cn(
            'h-full',
            skipTransition ? '' : 'transition-[opacity,transform] duration-200 ease-out',
            previewExpanded ? 'opacity-100 translate-x-0' : 'opacity-0 translate-x-2'
          )}
        >
          <ClipboardPreviewPane entryId={previewEntryId} />
        </div>
      </div>
    </div>
  )
}

export default ClipboardHistoryPanel
