import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { StateSnapshot, VirtuosoHandle } from 'react-virtuoso'
import { favoriteClipboardItem, Filter, unfavoriteClipboardItem } from '@/api/clipboardItems'
import { restoreClipboardEntry } from '@/api/daemon'
import { toast } from '@/components/ui/toast'
import { useCopyFeedback } from '@/hooks/useCopyFeedback'
import { useDeleteFlow } from '@/hooks/useDeleteFlow'
import { useHistoryData } from '@/hooks/useHistoryData'
import { useSearchTags } from '@/hooks/useSearchTags'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { useTransferProgress } from '@/hooks/useTransferProgress'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import { copyToClipboard, removeClipboardItem } from '@/store/slices/clipboardSlice'
import { fetchSpaceMembers } from '@/store/slices/devicesSlice'
import {
  readHistorySessionSnapshot,
  updateHistorySessionSelection,
  writeHistorySessionSnapshot,
} from './historySessionSnapshot'

const log = createLogger('history-controller')

/**
 * Orchestration layer for the History page: owns the state shared across the
 * three regions (filter panel ↔ list ↔ preview) and the handlers/effects that
 * tie them together, so the page component itself is pure layout.
 *
 * The shared state lives here rather than inside the region components because
 * it genuinely spans them — the list and preview are bound by `selectedId`, the
 * filter panel and list by the filter register — so a common owner is the right
 * seam; splitting the regions into self-contained components would only push
 * this state into a context with no real decoupling gain.
 */
export function useHistoryController() {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()

  useShortcutScope('clipboard')
  // Activate the file-transfer progress event listener for this page.
  useTransferProgress()

  // Prime the paired-device list so the row context menu's "send to device"
  // submenu has names to show even when the user lands here before ever
  // opening the Devices page (which is the only other fetch site). Idempotent.
  useEffect(() => {
    // Surface transport failures instead of letting them masquerade as an empty
    // device list (which would silently hide the "send to device" submenu).
    dispatch(fetchSpaceMembers())
      .unwrap()
      .catch(err => {
        log.warn({ err }, 'failed to prime paired-device list')
      })
  }, [dispatch])

  const data = useHistoryData()
  const searchableTags = useSearchTags()
  const initialSnapshot = readHistorySessionSnapshot()

  // Per-card interaction state (small + render-driving).
  const [requestedHoveredId, setHoveredId] = useState<string | null>(null)
  // Stable Set of ids already rendered once; read during render to gate the
  // entrance animation, mutated only in an effect (never during render).
  const [seenIds] = useState(() => new Set<string>(initialSnapshot?.seenIds ?? []))
  const [scrollState, setScrollState] = useState<StateSnapshot | null>(
    () => initialSnapshot?.scrollState ?? null
  )
  const listRef = useRef<VirtuosoHandle>(null)
  // Optimistic favorite overrides keyed by entry id: the live list does not
  // re-fetch on toggle, so a flip is reflected here immediately and reverted if
  // the backend call fails.
  const [favoriteOverrides, setFavoriteOverrides] = useState<Record<string, boolean>>({})
  const searchInputRef = useRef<HTMLInputElement>(null)

  const { copySuccessId, promotedId, markCopied } = useCopyFeedback()

  // ── Copy handler ──────────────────────────────────────────────
  const handleCopy = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await dispatch(copyToClipboard(id)).unwrap()
        markCopied(id)
        return true
      } catch (err) {
        // `copyToClipboard` rejects with a specific, user-facing reason for
        // unrecoverable failures (e.g. PAYLOAD_UNAVAILABLE → the entry's bytes
        // are gone); surface it instead of flattening every failure into the
        // generic copy-failed toast. Fall back to the generic text for opaque
        // (non-string) errors.
        toast.error(typeof err === 'string' ? err : t('clipboard.errors.copyFailed'))
        return false
      }
    },
    [dispatch, t, markCopied]
  )

  const handleCopyFilePaths = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await restoreClipboardEntry(id, { filePathsOnly: true })
        markCopied(id)
        return true
      } catch {
        toast.error(t('clipboard.errors.copyFailed'))
        return false
      }
    },
    [markCopied, t]
  )

  // ── Delete flow ───────────────────────────────────────────────
  const removeEntry = useCallback(
    async (id: string) => {
      try {
        await dispatch(removeClipboardItem(id)).unwrap()
        data.removeItem(id)
      } catch {
        toast.error(t('clipboard.errors.deleteFailed', 'Delete failed'))
      }
    },
    [dispatch, t, data.removeItem]
  )

  const { deleteDialogOpen, setDeleteDialogOpen, deletingId, requestDelete, confirmDelete } =
    useDeleteFlow(removeEntry)

  // ── Favorite toggle ───────────────────────────────────────────
  // Optimistically flip the override, call the backend, and revert on failure.
  // `current` is passed by the card so the handler needs no list lookup.
  const handleToggleFavorite = useCallback(
    async (id: string, current: boolean) => {
      const next = !current
      setFavoriteOverrides(prev => ({ ...prev, [id]: next }))
      try {
        await (next ? favoriteClipboardItem(id) : unfavoriteClipboardItem(id))
      } catch {
        setFavoriteOverrides(prev => ({ ...prev, [id]: current }))
        toast.error(t('clipboard.errors.favoriteFailed'))
      }
    },
    [t]
  )

  // Float the just-copied entry to the front of the list, with optimistic
  // favorite overrides applied on top of the engine's `isFavorited`.
  const orderedItems = useMemo(() => {
    const hasOverrides = Object.keys(favoriteOverrides).length > 0
    const base = hasOverrides
      ? data.baseItems.map(it =>
          it.id in favoriteOverrides ? { ...it, isFavorited: favoriteOverrides[it.id] } : it
        )
      : data.baseItems
    if (!promotedId) return base
    const idx = base.findIndex(it => it.id === promotedId)
    if (idx <= 0) return base
    return [base[idx], ...base.slice(0, idx), ...base.slice(idx + 1)]
  }, [data.baseItems, promotedId, favoriteOverrides])

  const [selection, setSelection] = useState<{
    id: string | null
    items: DisplayClipboardItem[]
  }>(() => ({ id: initialSnapshot?.selectedId ?? null, items: [] }))

  if (selection.items !== orderedItems) {
    const ids = new Set(orderedItems.map(item => item.id))
    const firstItemId = orderedItems[0]?.id ?? null
    const previousFirstItemId = selection.items[0]?.id ?? null
    const previousIds = new Set(selection.items.map(item => item.id))
    let nextId = selection.id

    if (nextId !== null && !ids.has(nextId)) nextId = null
    if (firstItemId !== null && nextId === null) nextId = firstItemId
    if (
      previousFirstItemId !== null &&
      nextId === previousFirstItemId &&
      firstItemId !== previousFirstItemId &&
      !previousIds.has(firstItemId)
    ) {
      nextId = firstItemId
    }

    setSelection({ id: nextId, items: orderedItems })
  }

  const selectedId = selection.id
  const orderedItemIds = new Set(orderedItems.map(item => item.id))
  const hoveredId =
    requestedHoveredId !== null && orderedItemIds.has(requestedHoveredId)
      ? requestedHoveredId
      : null

  const selectedItem = useMemo(
    () => orderedItems.find(it => it.id === selectedId) ?? null,
    [orderedItems, selectedId]
  )

  // ── Hover keyboard shortcuts ──────────────────────────────────
  useShortcut({
    key: 'c',
    scope: 'clipboard',
    enabled: hoveredId !== null,
    handler: () => {
      if (hoveredId) handleCopy(hoveredId)
    },
    preventDefault: false,
  })

  useShortcut({
    key: 'd',
    scope: 'clipboard',
    enabled: hoveredId !== null,
    handler: () => {
      if (hoveredId) requestDelete(hoveredId)
    },
    preventDefault: false,
  })

  useShortcut({
    key: 'f',
    id: 'clipboard.favorite',
    scope: 'clipboard',
    enabled: selectedItem !== null,
    handler: () => {
      if (!selectedItem) return
      void handleToggleFavorite(selectedItem.id, selectedItem.isFavorited === true)
    },
    preventDefault: false,
  })

  // CMD/Ctrl+F focuses the search box (works even while another input is focused).
  useShortcut({
    key: 'mod+f',
    scope: 'clipboard',
    handler: () => {
      const el = searchInputRef.current
      if (!el) return
      el.focus()
      el.select()
    },
    enableOnFormTags: true,
    preventDefault: true,
  })

  const handleCardClick = useCallback(
    (id: string) => setSelection({ id, items: orderedItems }),
    [orderedItems]
  )

  useEffect(() => {
    if (selectedId !== null) updateHistorySessionSelection(selectedId)
  }, [selectedId])

  const writeCurrentSnapshot = useEffectEvent((nextScrollState: StateSnapshot) => {
    writeHistorySessionSnapshot({
      searchState: data.filter,
      live: data.liveSnapshot,
      selectedId,
      seenIds: Array.from(seenIds),
      scrollState: nextScrollState,
    })
  })

  useEffect(() => {
    return () => {
      listRef.current?.getState(nextScrollState => {
        writeCurrentSnapshot(nextScrollState)
      })
    }
  }, [])

  // Heading reflects the active quick view / content-type filter ("收藏",
  // "文本", …), falling back to "全部" while unfiltered.
  const viewLabel =
    data.filter.tagFilter !== null
      ? t(`history.type.${data.filter.tagFilter}`, { defaultValue: data.filter.tagFilter })
      : data.filter.activeFilter === Filter.All
        ? t('history.filter.all')
        : data.filter.activeFilter === Filter.Favorited
          ? t('history.filter.favorited')
          : t(`history.type.${data.filter.activeFilter}`)

  return {
    // Filter / search register (shared by the search bar and the filter panel).
    filter: data.filter,
    filterActions: data.actions,
    sourceOptions: data.sourceOptions,
    searchableTags,
    browseCount: data.browseCount,
    indexState: data.indexState,
    isSearchActive: data.isSearchActive,
    searchLoading: data.searchLoading,
    searchInputRef,
    viewLabel,

    // List region.
    items: orderedItems,
    seenIds,
    listRef,
    scrollState,
    setScrollState,
    hasMore: data.hasMore,
    handleLoadMore: data.handleLoadMore,
    hoveredId,
    setHoveredId,
    selectedId,
    copySuccessId,
    deletingId,

    // Preview region.
    selectedItem,

    // Cross-region handlers.
    handleCopy,
    handleCopyFilePaths,
    requestDelete,
    handleToggleFavorite,
    handleCardClick,

    // Delete confirmation dialog.
    deleteDialogOpen,
    setDeleteDialogOpen,
    confirmDelete,
  }
}
