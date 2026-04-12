import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import React, { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import ClipboardPreviewPane from './ClipboardPreviewPane'
import HistoryPane from './components/HistoryPane'
import { PREVIEW_OPEN_DELAY_MS, PREVIEW_SWITCH_DELAY_MS } from './constants'
import { useHistorySearch } from './hooks/useHistorySearch'
import type { DisplayItem, PreviewAction, PreviewState, TimeRangePreset } from './types'
import { Filter } from '@/api/clipboardItems'
import { deleteClipboardEntry, restoreClipboardEntry } from '@/api/daemon'
import { unlockEncryptionSession } from '@/api/security'
import { useClipboardCollection } from '@/hooks/useClipboardCollection'
import { useDebounce } from '@/hooks/useDebounce'
import { useThemeSync } from '@/hooks/useThemeSync'
import { getItemPreview, resolveItemType } from '@/lib/clipboard-utils'
import { createLogger } from '@/lib/logger'
import { readStoredUiScale, subscribeUiScaleChanges } from '@/lib/ui-scale'
import { cn } from '@/lib/utils'

const log = createLogger('clipboard-history-panel')

async function dismissPanel(): Promise<void> {
  await invoke('dismiss_quick_panel')
}

async function pasteToApp(): Promise<void> {
  await invoke('paste_to_previous_app')
}

async function setQuickPanelLayout(scale: number, previewExpanded: boolean): Promise<void> {
  await invoke('set_quick_panel_layout', { scale, previewExpanded })
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
      void setQuickPanelLayout(readStoredUiScale(), false).catch(() => {})
    },
    [clearPreviewTimer]
  )

  useEffect(() => {
    const unlistenPrepare = listen('quick-panel://prepare-show', () => {
      setSkipTransition(true)
      clearPreviewTimer()
      previewLayoutTokenRef.current += 1
      dispatchPreview({ type: 'reset' })
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

      setTimeout(() => {
        setSkipTransition(false)
        void setQuickPanelLayout(readStoredUiScale(), false)
          .then(() => invoke('finalize_quick_panel_show'))
          .then(() => searchInputRef.current?.focus())
          .catch(() => {})
      }, 0)
    })

    return () => {
      clearPreviewTimer()
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

  useEffect(() => {
    if (deletingRef.current) {
      deletingRef.current = false
      return
    }
    setSelectedIndex(0)
  }, [filteredItems.length])

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
        dispatchPreview({
          type: 'reserve-space',
          entryId: nextEntryId,
          historyLockedWidth: nextHistoryWidth > 0 ? nextHistoryWidth : null,
        })
        void setQuickPanelLayout(uiScale, true)
          .then(() => {
            if (previewLayoutTokenRef.current === token) dispatchPreview({ type: 'expand' })
          })
          .catch(() => {
            if (previewLayoutTokenRef.current === token) dispatchPreview({ type: 'reset' })
          })
      },
      previewEntryId ? PREVIEW_SWITCH_DELAY_MS : PREVIEW_OPEN_DELAY_MS
    )
    return clearPreviewTimer
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
    async (index: number) => {
      const item = filteredItems[index]
      if (!item) return
      try {
        await restoreClipboardEntry(item.id)
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
        if (index < filteredItems.length) void handleSelect(index)
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
          void handleSelect(selectedIndex)
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

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-transparent p-4">
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
        />
      </div>

      <div
        className={cn(
          'min-w-0',
          previewExpanded
            ? 'ml-2 flex-1 basis-0 opacity-100 translate-x-0'
            : previewReservingSpace && historyLockedWidth != null
              ? 'ml-2 shrink-0 opacity-0 translate-x-0 pointer-events-none'
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
