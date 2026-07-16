import React, {
  useCallback,
  useEffect,
  useEffectEvent,
  useMemo,
  useReducer,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'
import { Filter, favoriteClipboardItem, unfavoriteClipboardItem } from '@/api/clipboardItems'
import { deleteClipboardEntry, restoreClipboardEntry } from '@/api/daemon'
import { unlockEncryptionSession } from '@/api/security'
import { toast } from '@/components/ui/toast'
import { useDebounce } from '@/hooks/useDebounce'
import { useHistorySourceOptions } from '@/hooks/useHistorySourceOptions'
import { useSearchTags } from '@/hooks/useSearchTags'
import { useThemeSync } from '@/hooks/useThemeSync'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { readStoredUiScale, subscribeUiScaleChanges } from '@/lib/ui-scale'
import { playUiSound } from '@/lib/ui-sound'
import { cn } from '@/lib/utils'
import { useAppDispatch } from '@/store/hooks'
import { fetchSpaceMembers } from '@/store/slices/devicesSlice'
import ClipboardPreviewPane from './ClipboardPreviewPane'
import HistoryPane from './components/HistoryPane'
import { PREVIEW_OPEN_DELAY_MS, PREVIEW_SWITCH_DELAY_MS, QUICK_FILTER_ORDER } from './constants'
import { useHistorySearch } from './hooks/useHistorySearch'
import type {
  PreviewAction,
  PreviewSide,
  PreviewState,
  QuickPanelContextMenuActions,
  TimeRangePreset,
} from './types'

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
  side: 'right',
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
        side: state.side,
      }
    case 'set-side':
      return state.side === action.side ? state : { ...state, side: action.side }
    case 'expand':
      if (state.mode === 'expanded') return state
      return { ...state, mode: 'expanded', historyLockedWidth: null }
    default:
      return state
  }
}

interface ClipboardHistoryPanelProps {
  showRequestId?: number
  onShowPrepared?: (requestId: number) => void
}

/**
 * Outer shell: remount the session on every `showRequestId` so search buffer,
 * filters, and the live-search item list cannot flash the previous show's
 * results. The webview stays alive between hides; only this tree is recreated.
 */
const ClipboardHistoryPanel: React.FC<ClipboardHistoryPanelProps> = props => {
  useThemeSync()
  return <ClipboardHistoryPanelSession key={props.showRequestId ?? 0} {...props} />
}

const ClipboardHistoryPanelSession: React.FC<ClipboardHistoryPanelProps> = ({
  showRequestId = 0,
  onShowPrepared,
}) => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()

  // Refresh the paired-device list so the row context menu's "send to device"
  // submenu shows current names and connection state. The quick panel is a
  // separate webview with its own store and — unlike the main window's
  // DevicesPage — does not subscribe to `peers.changed`, so this snapshot is the
  // only source. Fresh on every session mount (each re-open).
  useEffect(() => {
    dispatch(fetchSpaceMembers())
      .unwrap()
      .catch(err => {
        log.warn({ err }, 'failed to prime paired-device list')
      })
  }, [dispatch])

  const [searchQuery, setSearchQuery] = useState('')
  // Debounce typing only; clearing must hit the data layer immediately so the
  // list does not keep showing matches for ~300ms after the box is empty. The
  // `.trim()` here only gates empty-vs-not (whitespace-only reads as cleared);
  // `useHistorySearch` owns the actual query normalization for the daemon call.
  const debouncedSearchQuery = useDebounce(searchQuery, 300)
  const activeSearchQuery = searchQuery.trim() === '' ? '' : debouncedSearchQuery
  const [activeFilter, setActiveFilter] = useState<Filter>(Filter.All)
  const [tagFilter, setTagFilter] = useState<string | null>(null)
  const [sourceFilter, setSourceFilter] = useState<string | null>(null)
  const [extensionFilter, setExtensionFilter] = useState<string | null>(null)
  const [timeRange, setTimeRange] = useState<TimeRangePreset>('all_time')
  const searchableTags = useSearchTags()
  const sourceOptions = useHistorySourceOptions()

  const [selectedIndex, setSelectedIndex] = useState(0)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const [isKeyboardNav, setIsKeyboardNav] = useState(true)
  const [unlocking, setUnlocking] = useState(false)
  const [unlockError, setUnlockError] = useState<string | null>(null)
  const [uiScale, setUiScale] = useState(() => readStoredUiScale())
  const [previewState, dispatchPreview] = useReducer(previewReducer, initialPreviewState)
  const [hasPointerMovedSinceShow, setHasPointerMovedSinceShow] = useState(false)
  // Optimistic favorite overrides keyed by entry id, mirroring the history
  // controller: the live list does not re-fetch on toggle, so a flip is
  // reflected here immediately (and reverted if the backend call fails) — this
  // keeps the context menu's Favorite/Unfavorite label correct on re-open.
  const [favoriteOverrides, setFavoriteOverrides] = useState<Record<string, boolean>>({})

  const searchInputRef = useRef<HTMLInputElement>(null)
  const historyPaneRef = useRef<HTMLDivElement>(null)
  const itemRefs = useRef<Map<number, HTMLDivElement>>(new Map())
  const previewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const previewLayoutTokenRef = useRef(0)
  const [skipTransition, setSkipTransition] = useState(false)
  const previewExpanded = previewState.mode === 'expanded'
  const previewReservingSpace = previewState.mode === 'reserving'
  const previewEntryId = previewState.entryId
  const previewSuppressed = previewState.suppressed
  const historyLockedWidth = previewState.historyLockedWidth
  const previewFocusSource = previewState.focusSource
  const previewSide = previewState.side

  const { filteredItems, previewItems, isSearching, searchTotal, loading, isLocked, removeItem } =
    useHistorySearch({
      searchQuery: activeSearchQuery,
      activeFilter,
      tagFilter,
      sourceFilter,
      extensionFilter,
      timeRange,
    })
  const visibleUnlocking = isLocked && unlocking
  const visibleUnlockError = isLocked ? unlockError : null
  const [previousFilteredCount, setPreviousFilteredCount] = useState(filteredItems.length)
  const [preserveSelectionOnNextListChange, setPreserveSelectionOnNextListChange] = useState(false)

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

  // Session remount already resets search/filters/list. This only focuses the
  // search box and tells the host the UI is ready to finalize the show.
  const notifyShowPrepared = useEffectEvent((requestId: number) => {
    onShowPrepared?.(requestId)
  })
  useEffect(() => {
    if (showRequestId === 0) return
    setSkipTransition(true)

    const focusTimer = setTimeout(() => {
      setSkipTransition(false)
      notifyShowPrepared(showRequestId)
      searchInputRef.current?.focus()
    }, 0)

    return () => {
      clearTimeout(focusTimer)
    }
  }, [showRequestId])

  useEffect(() => {
    void setQuickPanelLayout(uiScale, previewExpanded).catch(() => {})
  }, [previewExpanded, uiScale])

  useEffect(() => subscribeUiScaleChanges(setUiScale), [])

  useEffect(() => {
    const handleWindowKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      // A right-click menu is open: let Radix consume Escape (close the
      // menu/submenu) rather than dismissing the whole panel. Both listen on the
      // same bubbling keydown and Radix does not stop propagation, so without
      // this guard one Escape would close the menu AND the panel.
      if (document.querySelector('[data-slot="context-menu-content"][data-state="open"]')) return
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
      // The live list re-queries itself once the session reports ready (the
      // base query effect re-runs on `encryptionReady`), so no manual reload.
    } catch (err) {
      setUnlocking(false)
      setUnlockError(err instanceof Error ? err.message : String(err))
    }
  }, [])

  useEffect(() => {
    if (isLocked) closePreview(false)
  }, [closePreview, isLocked])

  // Reset selectedIndex in the same render that receives a different visible
  // result count, so a new filter never paints the stale selected row.
  if (previousFilteredCount !== filteredItems.length) {
    setPreviousFilteredCount(filteredItems.length)
    if (preserveSelectionOnNextListChange) {
      setPreserveSelectionOnNextListChange(false)
    } else if (selectedIndex !== 0) {
      setSelectedIndex(0)
    }
  }

  useEffect(() => {
    const el = itemRefs.current.get(selectedIndex)
    el?.scrollIntoView?.({ block: 'nearest' })
  }, [selectedIndex])

  const selectedItem = filteredItems[selectedIndex] ?? null
  const hoveredItem = hoveredIndex != null ? (filteredItems[hoveredIndex] ?? null) : null
  const previewTargetId =
    previewFocusSource === 'hover'
      ? (hoveredItem?.id ?? previewEntryId)
      : (selectedItem?.id ?? null)
  const targetPreviewItem =
    previewTargetId != null
      ? (previewItems.find(item => item.id === previewTargetId) ?? null)
      : null
  const previewItem = previewEntryId
    ? (previewItems.find(item => item.id === previewEntryId) ?? null)
    : null

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
          dispatchPreview({ type: 'set-side', side })
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
        playUiSound('success')
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
        setPreserveSelectionOnNextListChange(true)
        clearPreviewTimer()
        setHoveredIndex(null)
        dispatchPreview({ type: 'suppress', value: false })
        dispatchPreview({ type: 'set-focus-source', source: 'selection' })
        const remainingItems = filteredItems.filter((_, i) => i !== index)
        const nextIndex = remainingItems.length > 0 ? Math.min(index, remainingItems.length - 1) : 0
        setSelectedIndex(nextIndex)
        removeItem(item.id)
      } catch (err) {
        log.error({ err }, 'Failed to delete clipboard entry')
      }
    },
    [clearPreviewTimer, filteredItems, removeItem]
  )

  // ── Right-click context menu (reuses the history menu) ──────────────
  // The shared `HistoryCardContextMenu` is id-based, so these adapt the panel's
  // own data layer to it. Copy dismisses the panel (launcher terminal action),
  // favorite flips optimistically, and delete reuses `handleDelete` by resolving
  // the id back to its list index (immediate, no confirm — matches Alt+Backspace).
  const handleContextCopy = useCallback(
    async (id: string) => {
      try {
        await restoreClipboardEntry(id)
        playUiSound('success')
      } catch (err) {
        log.error({ err }, 'Failed to copy clipboard entry')
        toast.error(t('clipboard.errors.copyFailed'))
        return
      }
      // Terminal launcher action: dismiss only after the copy actually landed.
      // The copy already succeeded, so a dismiss failure must not surface as a
      // copy error; swallow it with a warning rather than leaving the rejection
      // unhandled (the caller discards this promise via `void`).
      await dismissPanel().catch(err => {
        log.warn({ err }, 'failed to dismiss quick panel after copy')
      })
    },
    [t]
  )

  const handlePasteFilePaths = useCallback(
    async (id: string) => {
      try {
        await restoreClipboardEntry(id, { filePathsOnly: true })
        await pasteToApp()
        playUiSound('success')
      } catch (err) {
        log.error({ err }, 'Failed to paste file paths')
        toast.error(t('clipboard.errors.copyFailed'))
      }
    },
    [t]
  )

  const handleContextToggleFavorite = useCallback(
    async (id: string, current: boolean) => {
      const next = !current
      setFavoriteOverrides(prev => ({ ...prev, [id]: next }))
      try {
        await (next ? favoriteClipboardItem(id) : unfavoriteClipboardItem(id))
      } catch (err) {
        setFavoriteOverrides(prev => ({ ...prev, [id]: current }))
        log.error({ err }, 'Failed to toggle favorite')
        toast.error(t('clipboard.errors.favoriteFailed'))
      }
    },
    [t]
  )

  const handleContextDelete = useCallback(
    (id: string) => {
      const index = filteredItems.findIndex(item => item.id === id)
      if (index >= 0) void handleDelete(index)
    },
    [filteredItems, handleDelete]
  )

  // Right-clicking a row moves the selection onto it, so the highlighted row is
  // always the one the menu acts on (otherwise the keyboard-selected row stays
  // highlighted while the menu targets a different, right-clicked row). Preview
  // follows the selection; Radix opens the menu on the same event.
  const handleContextMenuSelect = useCallback((index: number) => {
    dispatchPreview({ type: 'suppress', value: false })
    dispatchPreview({ type: 'set-focus-source', source: 'selection' })
    setHoveredIndex(null)
    setSelectedIndex(index)
  }, [])

  // Rich items backing the menu, index-aligned with `filteredItems` (both derive
  // from the same live list), with optimistic favorite flips applied on top.
  const contextItems = useMemo(() => {
    if (Object.keys(favoriteOverrides).length === 0) return previewItems
    return previewItems.map(item =>
      item.id in favoriteOverrides ? { ...item, isFavorited: favoriteOverrides[item.id] } : item
    )
  }, [previewItems, favoriteOverrides])

  const contextActions = useMemo<QuickPanelContextMenuActions>(
    () => ({
      onCopy: id => void handleContextCopy(id),
      onPasteFilePaths: id => void handlePasteFilePaths(id),
      onToggleFavorite: (id, current) => void handleContextToggleFavorite(id, current),
      onDelete: handleContextDelete,
    }),
    [handleContextCopy, handleContextDelete, handleContextToggleFavorite, handlePasteFilePaths]
  )

  const handleSearchChange = useCallback((value: string) => {
    dispatchPreview({ type: 'suppress', value: false })
    dispatchPreview({ type: 'set-focus-source', source: 'selection' })
    setHoveredIndex(null)
    setSearchQuery(value)
  }, [])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (isLocked) {
        if (e.key === 'Enter' && !visibleUnlocking) {
          e.preventDefault()
          void handleUnlock()
        }
        return
      }

      // Hover takes priority over the keyboard-selected row: the two are
      // mutually exclusive in practice (arrow/Ctrl+n/p navigation clears
      // hover), so this always resolves to the row the user is currently
      // pointing at or has highlighted.
      const activeIndex = hoveredIndex ?? selectedIndex

      if (e.altKey && e.key === 'Backspace') {
        e.preventDefault()
        void handleDelete(activeIndex)
        return
      }

      if ((e.metaKey || e.ctrlKey) && e.key >= '0' && e.key <= '9') {
        e.preventDefault()
        const index = e.key === '0' ? 9 : parseInt(e.key) - 1
        if (index < filteredItems.length) void handleSelect(index, e.altKey)
        return
      }

      // Ctrl/Cmd+C copies the hovered/selected row to the system clipboard and
      // dismisses the panel, matching the row context menu's Copy action
      // (`handleContextCopy`) so the two entry points behave identically. Only
      // takes over when the search input has no text selection — with a
      // selection the user is copying part of their query, which is the
      // input's native behavior and must not be hijacked.
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'c') {
        const input = e.currentTarget
        if (input.selectionStart !== input.selectionEnd) return
        e.preventDefault()
        const item = filteredItems[activeIndex]
        if (item) void handleContextCopy(item.id)
        return
      }

      // Ctrl/Cmd+V is an alias for Enter: paste the hovered/selected row to the
      // foreground app via the same `handleSelect` path. Only takes over when
      // the search input is empty — once the user has typed a query, Cmd/Ctrl+V
      // is expected to paste text into the search box, not act on a row.
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'v') {
        if (e.currentTarget.value !== '') return
        e.preventDefault()
        void handleSelect(activeIndex, e.altKey)
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
      // the search input. The shared search box forwards unhandled Tab here, and
      // the shortcut writes into the same content-filter register as `type:`.
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
          void handleSelect(activeIndex, e.altKey)
          break
      }
    },
    [
      filteredItems,
      handleContextCopy,
      handleDelete,
      handleSelect,
      handleUnlock,
      hoveredIndex,
      isLocked,
      selectedIndex,
      visibleUnlocking,
    ]
  )

  const handleHistoryMouseMove = useCallback(() => setHasPointerMovedSinceShow(true), [])

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
          onContextMenuSelect={handleContextMenuSelect}
          onUnlock={handleUnlock}
          searchInputRef={searchInputRef}
          searchQuery={searchQuery}
          selectedIndex={selectedIndex}
          setHoveredIndex={setHoveredIndex}
          setIsKeyboardNav={setIsKeyboardNav}
          unlocking={visibleUnlocking}
          unlockError={visibleUnlockError}
          activeFilter={activeFilter}
          setActiveFilter={setActiveFilter}
          tagFilter={tagFilter}
          setTagFilter={setTagFilter}
          sourceFilter={sourceFilter}
          setSourceFilter={setSourceFilter}
          extensionFilter={extensionFilter}
          setExtensionFilter={setExtensionFilter}
          timeRange={timeRange}
          setTimeRange={setTimeRange}
          searchableTags={searchableTags}
          sourceOptions={sourceOptions}
          onKeyDown={handleKeyDown}
          contextItems={contextItems}
          contextActions={contextActions}
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
          <ClipboardPreviewPane item={previewItem} />
        </div>
      </div>
    </div>
  )
}

export default ClipboardHistoryPanel
