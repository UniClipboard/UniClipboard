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
import { applyThemePreset, DEFAULT_THEME_COLOR } from '@/lib/theme-engine'
import type { ThemeMode } from '@/lib/theme-engine'
import type { ClipboardEvent, SettingChangedEvent } from '@/types/events'
import type { Settings } from '@/types/setting'

// ── Types ──────────────────────────────────────────────────────────────

interface ClipboardEntryProjection {
  id: string
  preview: string
  has_detail: boolean
  size_bytes: number
  captured_at: number
  content_type: string
  is_encrypted: boolean
  is_favorited: boolean
  updated_at: number
  active_time: number
  thumbnail_url?: string | null
  link_urls?: string[] | null
  link_domains?: string[] | null
}

type ClipboardEntriesResponse =
  | { status: 'ready'; entries: ClipboardEntryProjection[] }
  | { status: 'not_ready' }

type ItemType = 'text' | 'image' | 'link' | 'code' | 'file' | 'unknown'

interface DisplayItem {
  id: string
  type: ItemType
  preview: string
  time: string
  activeTime: number
  isFavorited: boolean
}

// ── Helpers ────────────────────────────────────────────────────────────

function isImageType(contentType: string): boolean {
  return contentType === 'image' || contentType.startsWith('image/')
}

function resolveType(entry: ClipboardEntryProjection): ItemType {
  if (isImageType(entry.content_type)) return 'image'
  if (entry.link_urls && entry.link_urls.length > 0) return 'link'
  return 'text'
}

function getPreview(entry: ClipboardEntryProjection): string {
  const type = resolveType(entry)
  switch (type) {
    case 'image':
      return 'Image'
    case 'link':
      return entry.link_urls?.[0] ?? entry.preview
    default:
      return entry.preview
  }
}

function formatRelativeTime(timestampMs: number): string {
  const now = Date.now()
  const diffMs = now - timestampMs
  const diffMins = Math.round(diffMs / 60000)

  if (diffMins < 1) return 'just now'
  if (diffMins < 60) return `${diffMins}m`
  if (diffMins < 1440) return `${Math.floor(diffMins / 60)}h`
  return `${Math.floor(diffMins / 1440)}d`
}

const typeIcons: Record<ItemType, React.ElementType> = {
  text: FileText,
  image: ImageIcon,
  link: ExternalLink,
  code: Code,
  file: File,
  unknown: FileText,
}

// ── Theme sync ─────────────────────────────────────────────────────────

/**
 * Resolve the effective theme mode from settings, respecting 'system' preference.
 *
 * 根据设置解析实际的主题模式，支持跟随系统。
 */
function resolveThemeMode(theme: string | undefined | null): ThemeMode {
  if (theme === 'light' || theme === 'dark') return theme
  // 'system' or undefined — follow OS preference
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

/**
 * Apply the full theme (mode + color preset) to the document root.
 *
 * 将完整主题（模式 + 颜色预设）应用到文档根元素。
 */
function applyFullTheme(settings: Settings | null): void {
  const root = document.documentElement
  const theme = settings?.general?.theme
  const themeColor = settings?.general?.theme_color || DEFAULT_THEME_COLOR

  const resolvedMode = resolveThemeMode(theme)

  // Toggle dark class — same logic as SettingContext
  root.classList.remove('light', 'dark')
  root.classList.add(resolvedMode)

  // Apply theme color tokens
  applyThemePreset(themeColor, resolvedMode, root)
}

// ── Encryption status ─────────────────────────────────────────────────

async function checkEncryptionLocked(): Promise<boolean> {
  const status = await invoke<{ initialized: boolean; session_ready: boolean }>(
    'get_encryption_session_status'
  )
  return status.initialized && !status.session_ready
}

// ── Data fetch ─────────────────────────────────────────────────────────

async function fetchEntries(): Promise<DisplayItem[]> {
  const response = await invoke<ClipboardEntriesResponse>('get_clipboard_entries', {
    limit: 50,
    offset: 0,
  })

  if (response.status === 'not_ready') return []

  return response.entries.map(entry => ({
    id: entry.id,
    type: resolveType(entry),
    preview: getPreview(entry),
    time: formatRelativeTime(entry.active_time),
    activeTime: entry.active_time,
    isFavorited: entry.is_favorited,
  }))
}

async function restoreEntry(entryId: string): Promise<void> {
  await invoke('restore_clipboard_entry', { entryId })
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
        {item.time}
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
  const [items, setItems] = useState<DisplayItem[]>([])
  const [loading, setLoading] = useState(true)
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const [isKeyboardNav, setIsKeyboardNav] = useState(true)
  const [isLocked, setIsLocked] = useState(false)
  const [unlocking, setUnlocking] = useState(false)
  const [unlockError, setUnlockError] = useState<string | null>(null)

  const searchInputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const itemRefs = useRef<Map<number, HTMLDivElement>>(new Map())
  const previewTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const deletingRef = useRef(false)
  const visibleRef = useRef(false)
  const throttleTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const lastReloadTimestampRef = useRef<number | undefined>(undefined)

  // ── Theme sync with main window settings ──
  const settingsRef = useRef<Settings | null>(null)

  useEffect(() => {
    // 1. Load settings from backend and apply theme
    async function loadAndApplyTheme() {
      try {
        const settings = await invoke<Settings>('get_settings')
        settingsRef.current = settings
        applyFullTheme(settings)
      } catch (err) {
        console.error('Failed to load settings for theme:', err)
        // Fallback: apply default theme with system mode
        applyFullTheme(null)
      }
    }

    loadAndApplyTheme()

    // 2. Listen for settings changes from main window
    const unlistenSettings = listen<SettingChangedEvent>('setting-changed', event => {
      try {
        const newSettings = JSON.parse(event.payload.settingJson) as Settings
        settingsRef.current = newSettings
        applyFullTheme(newSettings)
      } catch (err) {
        console.error('Failed to parse setting-changed event:', err)
      }
    })

    // 3. Listen for OS theme changes (when theme mode is 'system')
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const handleSystemChange = () => {
      const settings = settingsRef.current
      if (!settings?.general?.theme || settings.general.theme === 'system') {
        applyFullTheme(settings)
      }
    }
    mq.addEventListener('change', handleSystemChange)

    return () => {
      unlistenSettings.then(fn => fn())
      mq.removeEventListener('change', handleSystemChange)
    }
  }, [])

  // Load data (check lock status first)
  const loadData = useCallback(async () => {
    setLoading(true)
    try {
      const locked = await checkEncryptionLocked()
      if (locked) {
        setIsLocked(true)
        return
      }
      setIsLocked(false)
      const entries = await fetchEntries()
      setItems(entries)
    } catch (err) {
      console.error('Failed to load clipboard entries:', err)
    } finally {
      setLoading(false)
    }
  }, [])

  // Load data on mount and when panel becomes visible
  useEffect(() => {
    loadData()

    // Listen for panel show event to reload data and re-focus search
    const unlisten = listen('quick-panel://refresh', () => {
      visibleRef.current = true
      setSearchQuery('')
      setSelectedIndex(0)
      setHoveredIndex(null)
      setIsKeyboardNav(true)
      invoke('dismiss_preview_panel').catch(() => {})
      loadData()
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
  }, [loadData])

  // Listen for encryption session ready event
  useEffect(() => {
    const unlistenPromise = listen<'SessionReady' | { type: string }>(
      'encryption://event',
      event => {
        const eventType = typeof event.payload === 'string' ? event.payload : event.payload?.type
        if (eventType === 'SessionReady') {
          setIsLocked(false)
          setUnlocking(false)
          setUnlockError(null)
          loadData()
        }
      }
    )

    return () => {
      unlistenPromise.then(fn => fn())
    }
  }, [loadData])

  // Live clipboard event updates (only when panel is visible)
  useEffect(() => {
    const unlistenPromise = listen<ClipboardEvent>('clipboard://event', event => {
      if (!visibleRef.current) return

      if (event.payload.type === 'NewContent' && event.payload.entry_id) {
        if (event.payload.origin === 'local') {
          // Fetch the single new entry and prepend
          invoke<ClipboardEntriesResponse>('get_clipboard_entry', {
            entryId: event.payload.entry_id,
          })
            .then(response => {
              if (response.status === 'not_ready' || response.entries.length === 0) return
              const entry = response.entries[0]
              const newItem: DisplayItem = {
                id: entry.id,
                type: resolveType(entry),
                preview: getPreview(entry),
                time: formatRelativeTime(entry.active_time),
                activeTime: entry.active_time,
                isFavorited: entry.is_favorited,
              }
              setItems(prev => [newItem, ...prev])
            })
            .catch(err => console.error('Failed to fetch new clipboard entry:', err))
        } else {
          // Remote event: throttled full reload
          const now = Date.now()
          const lastReload = lastReloadTimestampRef.current

          if (lastReload === undefined || now - lastReload >= 300) {
            lastReloadTimestampRef.current = now
            if (throttleTimeoutRef.current) {
              clearTimeout(throttleTimeoutRef.current)
              throttleTimeoutRef.current = null
            }
            loadData()
          } else if (!throttleTimeoutRef.current) {
            const delay = 300 - (now - lastReload)
            throttleTimeoutRef.current = setTimeout(() => {
              lastReloadTimestampRef.current = Date.now()
              loadData()
              throttleTimeoutRef.current = null
            }, delay)
          }
        }
      } else if (event.payload.type === 'Deleted' && event.payload.entry_id) {
        const deletedId = event.payload.entry_id
        setItems(prev => prev.filter(i => i.id !== deletedId))
      }
    })

    return () => {
      if (throttleTimeoutRef.current) {
        clearTimeout(throttleTimeoutRef.current)
        throttleTimeoutRef.current = null
      }
      unlistenPromise.then(fn => fn())
    }
  }, [loadData])

  // Unlock encryption session
  const handleUnlock = useCallback(async () => {
    setUnlocking(true)
    setUnlockError(null)
    try {
      await invoke('unlock_encryption_session')
      // SessionReady event will handle the rest
    } catch (err) {
      setUnlocking(false)
      setUnlockError(err instanceof Error ? err.message : String(err))
    }
  }, [])

  // Filter items by search query
  const filteredItems = useMemo(() => {
    if (!searchQuery.trim()) return items
    const q = searchQuery.toLowerCase()
    return items.filter(item => item.preview.toLowerCase().includes(q))
  }, [items, searchQuery])

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
        await restoreEntry(item.id)
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
        await invoke('delete_clipboard_entry', { entryId: item.id })

        // Mark as deleting so effects skip the dismiss/reset cycle
        deletingRef.current = true

        // Remove from local state
        setItems(prev => prev.filter(i => i.id !== item.id))

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
      } catch (err) {
        console.error('Failed to delete clipboard entry:', err)
      }
    },
    [filteredItems]
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
