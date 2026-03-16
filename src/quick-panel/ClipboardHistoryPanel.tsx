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
import { copyClipboardItem, deleteClipboardItem } from '@/api/clipboardItems'
import { unlockEncryptionSession } from '@/api/security'
import { useClipboardCollection } from '@/hooks/useClipboardCollection'
import { useThemeSync } from '@/hooks/useThemeSync'
import { formatRelativeTime, getItemPreview, resolveItemType } from '@/lib/clipboard-utils'
import type { ItemType } from '@/lib/clipboard-utils'

interface DisplayItem {
  id: string
  type: ItemType
  preview: string
  activeTime: number
  isFavorited: boolean
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

// ── Platform detection ─────────────────────────────────────────────────

const isMac = navigator.platform.toUpperCase().includes('MAC')

// ── Components ─────────────────────────────────────────────────────────

interface PanelItemProps {
  item: DisplayItem
  isSelected: boolean
  hoverDisabled: boolean
  onClick: () => void
  onMouseEnter: () => void
  itemRef?: React.Ref<HTMLDivElement>
  shortcutKey?: string
}

const PanelItem: React.FC<PanelItemProps> = ({
  item,
  isSelected,
  hoverDisabled,
  onClick,
  onMouseEnter,
  itemRef,
  shortcutKey,
}) => {
  const Icon = typeIcons[item.type] ?? FileText

  return (
    <div
      ref={itemRef}
      className={[
        'flex items-center gap-2.5 py-2 px-3 cursor-pointer select-none transition-colors',
        'rounded-md text-[13px] leading-tight',
        isSelected
          ? 'bg-primary text-primary-foreground'
          : hoverDisabled
            ? 'text-foreground'
            : 'hover:bg-accent text-foreground',
      ].join(' ')}
      onClick={onClick}
      onMouseEnter={onMouseEnter}
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
          'text-[11px] shrink-0 tabular-nums',
          isSelected ? 'text-primary-foreground/60' : 'text-muted-foreground',
        ].join(' ')}
      >
        {formatRelativeTime(item.activeTime)}
      </span>
      {shortcutKey && (
        <kbd
          className={[
            'text-[10px] leading-none px-1 py-0.5 rounded border shrink-0 font-mono',
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

// ── Main Panel ─────────────────────────────────────────────────────────

const ClipboardHistoryPanel: React.FC = () => {
  useThemeSync()

  const { items, loading, isLocked, reload } = useClipboardCollection()
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const [isKeyboardNav, setIsKeyboardNav] = useState(true)
  const [unlocking, setUnlocking] = useState(false)
  const [unlockError, setUnlockError] = useState<string | null>(null)

  const searchInputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const itemRefs = useRef<Map<number, HTMLDivElement>>(new Map())
  const previewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const deletingRef = useRef(false)
  const visibleRef = useRef(false)

  // Load data on mount and when panel becomes visible
  useEffect(() => {
    // Listen for panel show event to reload data and re-focus search
    const unlisten = listen('quick-panel://refresh', () => {
      visibleRef.current = true
      setSearchQuery('')
      setSelectedIndex(0)
      setHoveredIndex(null)
      setIsKeyboardNav(true)
      invoke('dismiss_preview_panel').catch(() => {})
      void reload()
      // Re-focus search input when panel is re-shown
      requestAnimationFrame(() => searchInputRef.current?.focus())
    })

    // Track visibility via focus/blur
    const unlistenFocus = listen('tauri://focus', () => {
      visibleRef.current = true
    })
    const unlistenBlur = listen('tauri://blur', () => {
      visibleRef.current = false
    })

    return () => {
      unlisten.then(fn => fn())
      unlistenFocus.then(fn => fn())
      unlistenBlur.then(fn => fn())
    }
  }, [reload])

  // Unlock encryption session
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
    }
  }, [isLocked])

  const displayItems = useMemo<DisplayItem[]>(
    () =>
      items.map(item => ({
        id: item.id,
        type: resolveItemType(item),
        preview: getItemPreview(item),
        activeTime: item.active_time,
        isFavorited: item.is_favorited,
      })),
    [items]
  )

  // Filter items by search query
  const filteredItems = useMemo(() => {
    if (!searchQuery.trim()) return displayItems
    const q = searchQuery.toLowerCase()
    return displayItems.filter(item => item.preview.toLowerCase().includes(q))
  }, [displayItems, searchQuery])

  // Reset selection when filter changes (but not during deletion)
  useEffect(() => {
    if (deletingRef.current) {
      deletingRef.current = false
      return
    }
    setSelectedIndex(0)
  }, [filteredItems.length])

  // Scroll selected item into view
  useEffect(() => {
    const el = itemRefs.current.get(selectedIndex)
    if (el) {
      el.scrollIntoView({ block: 'nearest' })
    }
  }, [selectedIndex])

  // Preview debounce: show preview after 500ms of hovering/selecting an item
  const focusedIndex = hoveredIndex ?? selectedIndex
  useEffect(() => {
    // Clear previous timer
    if (previewTimerRef.current) {
      clearTimeout(previewTimerRef.current)
      previewTimerRef.current = null
    }

    // Dismiss current preview immediately on focus change
    invoke('dismiss_preview_panel').catch(() => {})

    const focusedItem = filteredItems[focusedIndex]
    if (!focusedItem) return

    // Start new timer
    previewTimerRef.current = setTimeout(() => {
      invoke('show_preview_panel', { entryId: focusedItem.id }).catch(err => {
        console.error('Failed to show preview panel:', err)
      })
    }, 500)

    return () => {
      if (previewTimerRef.current) {
        clearTimeout(previewTimerRef.current)
        previewTimerRef.current = null
      }
    }
  }, [focusedIndex, filteredItems])

  // Select & paste item
  const handleSelect = useCallback(
    async (index: number) => {
      const item = filteredItems[index]
      if (!item) return

      try {
        await copyClipboardItem(item.id)
      } catch (err) {
        console.error('Failed to restore clipboard entry:', err)
        return
      }

      // Hide panel, activate previous app, and paste
      await pasteToApp()
    },
    [filteredItems]
  )

  // Delete selected item
  const handleDelete = useCallback(
    async (index: number) => {
      const item = filteredItems[index]
      if (!item) return

      try {
        await deleteClipboardItem(item.id)

        // Mark as deleting so effects skip the dismiss/reset cycle
        deletingRef.current = true

        // Stay at same index, or clamp to last item if we deleted the tail
        const newLength = filteredItems.length - 1
        const nextIndex = Math.min(index, newLength - 1)
        setSelectedIndex(nextIndex)

        // Immediately show preview for the next focused item
        const nextItem = filteredItems[index === filteredItems.length - 1 ? index - 1 : index + 1]
        if (nextItem) {
          invoke('show_preview_panel', { entryId: nextItem.id }).catch(() => {})
        } else {
          invoke('dismiss_preview_panel').catch(() => {})
        }
        void reload()
      } catch (err) {
        console.error('Failed to delete clipboard entry:', err)
      }
    },
    [filteredItems, reload]
  )

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // When locked, only allow Escape and Enter (unlock)
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

      // ⌥+Backspace: delete selected item
      if (e.altKey && e.key === 'Backspace') {
        e.preventDefault()
        handleDelete(selectedIndex)
        return
      }

      // ⌘/Ctrl + 1~0: quick paste the Nth item
      if ((e.metaKey || e.ctrlKey) && e.key >= '0' && e.key <= '9') {
        e.preventDefault()
        const index = e.key === '0' ? 9 : parseInt(e.key) - 1
        if (index < filteredItems.length) {
          handleSelect(index)
        }
        return
      }

      // Ctrl+N / Ctrl+P: Emacs-style navigation
      if (e.ctrlKey && (e.key === 'n' || e.key === 'p')) {
        e.preventDefault()
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
          setIsKeyboardNav(true)
          setHoveredIndex(null)
          setSelectedIndex(prev => Math.min(prev + 1, filteredItems.length - 1))
          break
        case 'ArrowUp':
          e.preventDefault()
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
          invoke('dismiss_preview_panel').catch(() => {})
          setHoveredIndex(null)
          dismissPanel()
          break
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [
    filteredItems.length,
    selectedIndex,
    handleSelect,
    handleDelete,
    isLocked,
    unlocking,
    handleUnlock,
  ])

  // Auto-focus search input
  useEffect(() => {
    searchInputRef.current?.focus()
  }, [])

  // Locked UI
  if (isLocked && !loading) {
    return (
      <div className="flex flex-col h-screen w-screen overflow-hidden rounded-xl bg-background/95 backdrop-blur-xl shadow-xl border border-border/50">
        <div className="flex-1 flex flex-col items-center justify-center gap-4 px-6">
          <div className="flex items-center justify-center w-12 h-12 rounded-xl bg-muted/30">
            <Lock className="h-6 w-6 text-muted-foreground" />
          </div>
          <div className="text-center space-y-1">
            <h2 className="text-sm font-medium text-foreground">Clipboard is locked</h2>
            <p className="text-[12px] text-muted-foreground">
              Unlock to access your clipboard history
            </p>
          </div>
          <button
            type="button"
            onClick={handleUnlock}
            disabled={unlocking}
            className="flex items-center gap-1.5 px-4 py-1.5 rounded-md text-[13px] font-medium bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 transition-colors"
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
            <p className="text-[12px] text-destructive text-center max-w-[15rem]">{unlockError}</p>
          )}
        </div>
        {/* Footer hint */}
        <div className="flex items-center justify-center px-3 py-1.5 border-t border-border/50 text-[11px] text-muted-foreground">
          <span>esc close</span>
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden rounded-xl bg-background/95 backdrop-blur-xl shadow-xl border border-border/50">
      {/* Search bar */}
      <div className="flex items-center gap-2 px-3 py-2.5 border-b border-border/50">
        <Search className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
        <input
          ref={searchInputRef}
          type="text"
          placeholder="Search clipboard history..."
          value={searchQuery}
          onChange={e => setSearchQuery(e.target.value)}
          className="flex-1 bg-transparent outline-none text-[13px] text-foreground placeholder:text-muted-foreground/60"
        />
        {searchQuery && (
          <span className="text-[11px] text-muted-foreground tabular-nums">
            {filteredItems.length}
          </span>
        )}
      </div>

      {/* Items list */}
      <div
        ref={listRef}
        className="flex-1 overflow-y-auto px-1.5 py-1 scrollbar-thin"
        onMouseMove={() => {
          if (isKeyboardNav) setIsKeyboardNav(false)
        }}
        onMouseLeave={() => setHoveredIndex(null)}
      >
        {loading ? (
          <div className="flex items-center justify-center h-full text-[13px] text-muted-foreground">
            Loading…
          </div>
        ) : filteredItems.length === 0 ? (
          <div className="flex items-center justify-center h-full text-[13px] text-muted-foreground">
            {searchQuery ? 'No matches' : 'No clipboard history'}
          </div>
        ) : (
          filteredItems.map((item, index) => (
            <PanelItem
              key={item.id}
              item={item}
              isSelected={index === selectedIndex}
              hoverDisabled={isKeyboardNav}
              onClick={() => handleSelect(index)}
              onMouseEnter={() => {
                if (!isKeyboardNav) setHoveredIndex(index)
              }}
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

      {/* Footer hint */}
      <div className="flex items-center justify-between px-3 py-1.5 border-t border-border/50 text-[11px] text-muted-foreground">
        <span>{isMac ? '⌘' : '⌃'}1-0 paste</span>
        <span>↑↓ navigate · ⏎ paste · {isMac ? '⌥' : 'Alt+'}⌫ delete · esc close</span>
      </div>
    </div>
  )
}

export default ClipboardHistoryPanel
