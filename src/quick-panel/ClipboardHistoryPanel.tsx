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
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import ClipboardPreviewPane from './ClipboardPreviewPane'
import { deleteClipboardEntry, restoreClipboardEntry } from '@/api/daemon'
import { unlockEncryptionSession } from '@/api/security'
import { useClipboardCollection } from '@/hooks/useClipboardCollection'
import { useThemeSync } from '@/hooks/useThemeSync'
import { formatRelativeTime, getItemPreview, resolveItemType } from '@/lib/clipboard-utils'
import type { ItemType } from '@/lib/clipboard-utils'

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

async function setPreviewExpanded(expanded: boolean): Promise<void> {
  await invoke('set_quick_panel_preview_expanded', { expanded })
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

const quickCardClassName =
  'flex h-screen w-[360px] min-w-[360px] max-w-[360px] flex-col overflow-hidden rounded-xl border border-border/50 bg-background/95 shadow-xl backdrop-blur-xl'

const ClipboardHistoryPanel: React.FC = () => {
  useThemeSync()

  const { items, loading, isLocked, reload } = useClipboardCollection()
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const [isKeyboardNav, setIsKeyboardNav] = useState(true)
  const [unlocking, setUnlocking] = useState(false)
  const [unlockError, setUnlockError] = useState<string | null>(null)
  const [previewEntryId, setPreviewEntryId] = useState<string | null>(null)
  const [previewSuppressed, setPreviewSuppressed] = useState(false)

  const searchInputRef = useRef<HTMLInputElement>(null)
  const itemRefs = useRef<Map<number, HTMLDivElement>>(new Map())
  const previewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const deletingRef = useRef(false)
  const [skipTransition, setSkipTransition] = useState(false)

  const clearPreviewTimer = useCallback(() => {
    if (previewTimerRef.current) {
      clearTimeout(previewTimerRef.current)
      previewTimerRef.current = null
    }
  }, [])

  const closePreview = useCallback(
    (suppressUntilNextSelection: boolean) => {
      clearPreviewTimer()
      setPreviewEntryId(null)
      setPreviewSuppressed(suppressUntilNextSelection)
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
      setPreviewEntryId(null)
      setPreviewSuppressed(false)
      setSearchQuery('')
      setSelectedIndex(0)
      setHoveredIndex(null)
      setIsKeyboardNav(true)
      void reload()

      // 2. Let React flush state updates, then make the window visible.
      //    Use setTimeout (not rAF — rAF doesn't fire while hidden).
      setTimeout(() => {
        setSkipTransition(false)
        void invoke('finalize_quick_panel_show')
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
    void setPreviewExpanded(Boolean(previewEntryId)).catch(() => {})
  }, [previewEntryId])

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

  const focusedIndex = hoveredIndex ?? selectedIndex
  const focusedItem = filteredItems[focusedIndex] ?? null
  useEffect(() => {
    clearPreviewTimer()

    if (previewSuppressed || isLocked) {
      return
    }

    if (!focusedItem) {
      setPreviewEntryId(null)
      return
    }

    if (previewEntryId === focusedItem.id) {
      return
    }

    previewTimerRef.current = setTimeout(
      () => {
        setPreviewEntryId(focusedItem.id)
      },
      previewEntryId ? PREVIEW_SWITCH_DELAY_MS : PREVIEW_OPEN_DELAY_MS
    )

    return clearPreviewTimer
  }, [
    clearPreviewTimer,
    focusedItem,
    isLocked,
    previewEntryId,
    previewSuppressed,
    selectedIndex,
    hoveredIndex,
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
      if (!isKeyboardNav) {
        setPreviewSuppressed(false)
        setHoveredIndex(index)
      }
    },
    [isKeyboardNav]
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
        setPreviewSuppressed(false)

        const remainingItems = filteredItems.filter((_, itemIndex) => itemIndex !== index)
        const nextIndex = remainingItems.length > 0 ? Math.min(index, remainingItems.length - 1) : 0
        const nextItem = remainingItems[nextIndex] ?? null

        setSelectedIndex(nextIndex)
        setPreviewEntryId(nextItem?.id ?? null)
        void reload()
      } catch (err) {
        console.error('Failed to delete clipboard entry:', err)
      }
    },
    [clearPreviewTimer, filteredItems, reload]
  )

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
        setPreviewSuppressed(false)
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
          setPreviewSuppressed(false)
          setIsKeyboardNav(true)
          setHoveredIndex(null)
          setSelectedIndex(prev => Math.min(prev + 1, filteredItems.length - 1))
          break
        case 'ArrowUp':
          e.preventDefault()
          setPreviewSuppressed(false)
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

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-transparent">
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
                onClick={handleUnlock}
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
                onChange={e => {
                  setPreviewSuppressed(false)
                  setSearchQuery(e.target.value)
                }}
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
                    onSelect={handleSelect}
                    onHover={handleHover}
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

            <div className="flex items-center justify-between border-t border-border/50 px-3 py-1.5 text-[11px] text-muted-foreground">
              <span>{isMac ? '⌘' : '⌃'}1-0 paste</span>
              <span>↑↓ navigate · ⏎ paste · {isMac ? '⌥' : 'Alt+'}⌫ delete · esc close</span>
            </div>
          </>
        )}
      </div>

      <div
        className={[
          'overflow-hidden',
          skipTransition ? '' : 'transition-all duration-200 ease-out',
          previewEntryId !== null
            ? 'ml-2 w-[360px] opacity-100 translate-x-0'
            : 'ml-0 w-0 opacity-0 translate-x-2 pointer-events-none',
        ].join(' ')}
        aria-hidden={previewEntryId === null}
      >
        <ClipboardPreviewPane entryId={previewEntryId} />
      </div>
    </div>
  )
}

export default ClipboardHistoryPanel
