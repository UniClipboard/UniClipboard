import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import {
  Code,
  ExternalLink,
  File,
  FileText,
  Image as ImageIcon,
  Loader2,
  Lock,
  Search,
  Unlock,
} from 'lucide-react'
import React, { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react'
import ClipboardPreviewPane from './ClipboardPreviewPane'
import { deleteClipboardEntry, restoreClipboardEntry } from '@/api/daemon'
import { unlockEncryptionSession } from '@/api/security'
import { useClipboardCollection } from '@/hooks/useClipboardCollection'
import { useThemeSync } from '@/hooks/useThemeSync'
import { formatRelativeTime, getItemPreview, resolveItemType } from '@/lib/clipboard-utils'
import type { ItemType } from '@/lib/clipboard-utils'
import { readStoredUiScale, subscribeUiScaleChanges } from '@/lib/ui-scale'

const PREVIEW_OPEN_DELAY_MS = 500
const PREVIEW_SWITCH_DELAY_MS = 120

interface DisplayItem {
  id: string
  type: ItemType
  preview: string
  activeTime: number
}

const typeIcons: Record<ItemType, React.ElementType> = {
  text: FileText,
  image: ImageIcon,
  link: ExternalLink,
  code: Code,
  file: File,
  unknown: FileText,
}

async function dismissPanel(): Promise<void> {
  await invoke('dismiss_quick_panel')
}

async function pasteToApp(): Promise<void> {
  await invoke('paste_to_previous_app')
}

async function setQuickPanelLayout(scale: number, previewExpanded: boolean): Promise<void> {
  await invoke('set_quick_panel_layout', { scale, previewExpanded })
}

const isMac = navigator.platform.toUpperCase().includes('MAC')

interface PanelItemProps {
  item: DisplayItem
  index: number
  isSelected: boolean
  hoverDisabled: boolean
  onSelect: (index: number) => void
  onHover: (index: number) => void
  itemRef?: React.Ref<HTMLDivElement>
  shortcutKey?: string
}

const PanelItem: React.FC<PanelItemProps> = React.memo(
  ({ item, index, isSelected, hoverDisabled, onSelect, onHover, itemRef, shortcutKey }) => {
    const Icon = typeIcons[item.type] ?? FileText

    return (
      <div
        ref={itemRef}
        className={[
          'flex cursor-pointer select-none items-center gap-2.5 rounded-md px-3 py-2 text-[13px] leading-tight',
          isSelected
            ? 'bg-primary text-primary-foreground'
            : hoverDisabled
              ? 'text-foreground'
              : 'text-foreground hover:bg-accent',
        ].join(' ')}
        onClick={() => onSelect(index)}
        onMouseEnter={() => onHover(index)}
      >
        <Icon
          className={[
            'h-3.5 w-3.5 shrink-0',
            isSelected ? 'text-primary-foreground/70' : 'text-muted-foreground',
          ].join(' ')}
        />
        <span className="flex-1 truncate">{item.preview || '(empty)'}</span>
        <span
          className={[
            'shrink-0 tabular-nums text-[11px]',
            isSelected ? 'text-primary-foreground/60' : 'text-muted-foreground',
          ].join(' ')}
        >
          {formatRelativeTime(item.activeTime)}
        </span>
        {shortcutKey && (
          <kbd
            className={[
              'shrink-0 rounded border px-1 py-0.5 font-mono text-[10px] leading-none',
              isSelected
                ? 'border-primary-foreground/30 text-primary-foreground/70'
                : 'border-border text-muted-foreground',
            ].join(' ')}
          >
            {isMac ? '⌘' : '⌃'}
            {shortcutKey}
          </kbd>
        )}
      </div>
    )
  }
)

interface HistoryPaneProps {
  filteredItems: DisplayItem[]
  hasPointerMovedSinceShow: boolean
  isKeyboardNav: boolean
  isLocked: boolean
  itemRefs: React.MutableRefObject<Map<number, HTMLDivElement>>
  loading: boolean
  onHover: (index: number) => void
  onHistoryMouseMove: () => void
  onSearchChange: (value: string) => void
  onSelect: (index: number) => void
  onUnlock: () => void
  searchInputRef: React.RefObject<HTMLInputElement | null>
  searchQuery: string
  selectedIndex: number
  setHoveredIndex: React.Dispatch<React.SetStateAction<number | null>>
  setIsKeyboardNav: React.Dispatch<React.SetStateAction<boolean>>
  unlocking: boolean
  unlockError: string | null
}

const HistoryPane: React.FC<HistoryPaneProps> = React.memo(
  ({
    filteredItems,
    hasPointerMovedSinceShow,
    isKeyboardNav,
    isLocked,
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
  }) => (
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
          <div className="flex items-center gap-2 border-b border-border/50 px-3 py-2.5">
            <Search className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            <input
              ref={searchInputRef}
              type="text"
              placeholder="Search clipboard history..."
              value={searchQuery}
              onChange={e => onSearchChange(e.target.value)}
              className="flex-1 bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground/60"
            />
            {searchQuery && (
              <span className="tabular-nums text-[11px] text-muted-foreground">
                {filteredItems.length}
              </span>
            )}
          </div>

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
                Loading…
              </div>
            ) : filteredItems.length === 0 ? (
              <div className="flex h-full items-center justify-center text-[13px] text-muted-foreground">
                {searchQuery ? 'No matches' : 'No clipboard history'}
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
                    if (el) {
                      itemRefs.current.set(index, el)
                    } else {
                      itemRefs.current.delete(index)
                    }
                  }}
                />
              ))
            )}
          </div>

          <div className="flex items-center justify-between gap-3 border-t border-border/50 px-3 py-1.5 text-[11px] text-muted-foreground">
            <span className="shrink-0">{isMac ? '⌘' : '⌃'}1-0 paste</span>
            <span className="min-w-0 flex-1 text-right truncate">
              ↑↓ navigate · ⏎ paste · {isMac ? '⌥' : 'Alt+'}⌫ delete · esc close
            </span>
          </div>
        </>
      )}
    </div>
  )
)

const quickCardClassName =
  'flex h-screen w-full min-w-0 flex-col overflow-hidden rounded-xl border border-border/50 bg-background/95 shadow-xl backdrop-blur-xl'

type PreviewMode = 'closed' | 'reserving' | 'expanded'
type PreviewFocusSource = 'selection' | 'hover'

interface PreviewState {
  entryId: string | null
  mode: PreviewMode
  suppressed: boolean
  historyLockedWidth: number | null
  focusSource: PreviewFocusSource
}

type PreviewAction =
  | { type: 'reset'; suppressed?: boolean }
  | { type: 'suppress'; value: boolean }
  | { type: 'set-entry'; entryId: string | null }
  | { type: 'set-focus-source'; source: PreviewFocusSource }
  | { type: 'reserve-space'; entryId: string; historyLockedWidth: number | null }
  | { type: 'expand' }

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
      return {
        ...initialPreviewState,
        suppressed: action.suppressed ?? false,
      }
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
      if (state.mode === 'expanded') {
        return state
      }
      return {
        ...state,
        mode: 'expanded',
        historyLockedWidth: null,
      }
  }
}

const ClipboardHistoryPanel: React.FC = () => {
  useThemeSync()

  const { items, loading, isLocked, reload } = useClipboardCollection()
  const [searchQuery, setSearchQuery] = useState('')
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

  // Two-phase show: the backend emits `prepare-show` instead of making
  // the window visible immediately.  We clear stale state, wait for the
  // browser to repaint, then call `finalize_quick_panel_show` so the
  // window becomes visible only after stale preview content is gone.
  useEffect(() => {
    const unlistenPrepare = listen('quick-panel://prepare-show', () => {
      // 1. Clear stale state (no CSS transition — instant)
      setSkipTransition(true)
      clearPreviewTimer()
      previewLayoutTokenRef.current += 1
      dispatchPreview({ type: 'reset' })
      setSearchQuery('')
      setSelectedIndex(0)
      setHoveredIndex(null)
      setIsKeyboardNav(true)
      setHasPointerMovedSinceShow(false)
      setPreviewTargetId(null)
      void reload()

      // 2. Let React flush state updates, then make the window visible.
      //    Use setTimeout (not rAF — rAF doesn't fire while hidden).
      setTimeout(() => {
        setSkipTransition(false)
        void setQuickPanelLayout(readStoredUiScale(), false)
          .then(() => invoke('finalize_quick_panel_show'))
          .then(() => {
            searchInputRef.current?.focus()
          })
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

  useEffect(() => {
    return subscribeUiScaleChanges(scale => {
      setUiScale(scale)
    })
  }, [])

  const handleUnlock = useCallback(async () => {
    setUnlocking(true)
    setUnlockError(null)
    try {
      await unlockEncryptionSession()
      setUnlocking(false)
      setUnlockError(null)
      void reload()
    } catch (err) {
      setUnlocking(false)
      setUnlockError(err instanceof Error ? err.message : String(err))
    }
  }, [reload])

  useEffect(() => {
    if (!isLocked) {
      setUnlocking(false)
      setUnlockError(null)
      return
    }

    closePreview(false)
  }, [closePreview, isLocked])

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

  const filteredItems = useMemo(() => {
    if (!searchQuery.trim()) return displayItems
    const q = searchQuery.toLowerCase()
    return displayItems.filter(item => item.preview.toLowerCase().includes(q))
  }, [displayItems, searchQuery])

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
    if (previewFocusSource !== 'selection') {
      return
    }

    setPreviewTargetId(selectedItem?.id ?? null)
  }, [previewFocusSource, selectedItem])

  useEffect(() => {
    if (previewFocusSource !== 'hover' && hoveredIndex == null) {
      return
    }

    if (!hoveredItem) {
      return
    }

    setPreviewTargetId(hoveredItem.id)
  }, [hoveredIndex, hoveredItem, previewFocusSource])

  useEffect(() => {
    clearPreviewTimer()

    if (previewSuppressed || isLocked) {
      return
    }

    if (!targetPreviewItem) {
      previewLayoutTokenRef.current += 1
      dispatchPreview({ type: 'reset' })
      void setQuickPanelLayout(uiScale, false).catch(() => {})
      return
    }

    if (previewEntryId === targetPreviewItem.id) {
      return
    }

    previewTimerRef.current = setTimeout(
      () => {
        const nextEntryId = targetPreviewItem.id
        dispatchPreview({ type: 'set-entry', entryId: nextEntryId })

        if (previewExpanded) {
          return
        }

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
            if (previewLayoutTokenRef.current !== token) {
              return
            }
            dispatchPreview({ type: 'expand' })
          })
          .catch(() => {
            if (previewLayoutTokenRef.current !== token) {
              return
            }
            dispatchPreview({ type: 'reset' })
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
      } catch (err) {
        console.error('Failed to restore clipboard entry:', err)
        return
      }

      await pasteToApp()
    },
    [filteredItems]
  )

  const handleHover = useCallback(
    (index: number) => {
      if (isKeyboardNav || !hasPointerMovedSinceShow) {
        return
      }

      const item = filteredItems[index]
      if (!item) {
        return
      }

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

        const remainingItems = filteredItems.filter((_, itemIndex) => itemIndex !== index)
        const nextIndex = remainingItems.length > 0 ? Math.min(index, remainingItems.length - 1) : 0
        const nextItem = remainingItems[nextIndex] ?? null

        setSelectedIndex(nextIndex)
        setPreviewTargetId(nextItem?.id ?? null)
        void reload()
      } catch (err) {
        console.error('Failed to delete clipboard entry:', err)
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

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (isLocked) {
        if (e.key === 'Escape') {
          e.preventDefault()
          dismissPanel()
        } else if (e.key === 'Enter' && !unlocking) {
          e.preventDefault()
          handleUnlock()
        }
        return
      }

      if (e.altKey && e.key === 'Backspace') {
        e.preventDefault()
        handleDelete(selectedIndex)
        return
      }

      if ((e.metaKey || e.ctrlKey) && e.key >= '0' && e.key <= '9') {
        e.preventDefault()
        const index = e.key === '0' ? 9 : parseInt(e.key) - 1
        if (index < filteredItems.length) {
          handleSelect(index)
        }
        return
      }

      if (e.ctrlKey && (e.key === 'n' || e.key === 'p')) {
        e.preventDefault()
        dispatchPreview({ type: 'suppress', value: false })
        dispatchPreview({ type: 'set-focus-source', source: 'selection' })
        setIsKeyboardNav(true)
        setHoveredIndex(null)
        if (e.key === 'n') {
          setSelectedIndex(prev => Math.min(prev + 1, filteredItems.length - 1))
        } else {
          setSelectedIndex(prev => Math.max(prev - 1, 0))
        }
        return
      }

      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault()
          dispatchPreview({ type: 'suppress', value: false })
          dispatchPreview({ type: 'set-focus-source', source: 'selection' })
          setIsKeyboardNav(true)
          setHoveredIndex(null)
          setSelectedIndex(prev => Math.min(prev + 1, filteredItems.length - 1))
          break
        case 'ArrowUp':
          e.preventDefault()
          dispatchPreview({ type: 'suppress', value: false })
          dispatchPreview({ type: 'set-focus-source', source: 'selection' })
          setIsKeyboardNav(true)
          setHoveredIndex(null)
          setSelectedIndex(prev => Math.max(prev - 1, 0))
          break
        case 'Enter':
          e.preventDefault()
          handleSelect(selectedIndex)
          break
        case 'Escape':
          e.preventDefault()
          setHoveredIndex(null)
          dismissPanel()
          break
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [
    filteredItems.length,
    handleDelete,
    handleSelect,
    handleUnlock,
    isLocked,
    selectedIndex,
    unlocking,
  ])

  useEffect(() => {
    searchInputRef.current?.focus()
  }, [])

  const handleHistoryMouseMove = useCallback(() => {
    setHasPointerMovedSinceShow(true)
  }, [])

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-transparent">
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
        />
      </div>

      <div
        className={[
          'min-w-0 overflow-hidden',
          previewExpanded
            ? 'ml-2 flex-1 basis-0 opacity-100 translate-x-0'
            : previewReservingSpace && historyLockedWidth != null
              ? 'ml-2 shrink-0 opacity-0 translate-x-0 pointer-events-none'
              : 'ml-0 w-0 opacity-0 translate-x-2 pointer-events-none',
        ].join(' ')}
        style={
          previewReservingSpace && historyLockedWidth != null
            ? { width: `max(0px, calc(100% - ${historyLockedWidth}px - 0.5rem))` }
            : undefined
        }
        aria-hidden={!previewExpanded}
      >
        <div
          className={[
            'h-full',
            skipTransition ? '' : 'transition-[opacity,transform] duration-200 ease-out',
            previewExpanded ? 'opacity-100 translate-x-0' : 'opacity-0 translate-x-2',
          ].join(' ')}
        >
          <ClipboardPreviewPane entryId={previewEntryId} />
        </div>
      </div>
    </div>
  )
}

export default ClipboardHistoryPanel
