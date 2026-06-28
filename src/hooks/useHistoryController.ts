import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { StateSnapshot, VirtuosoHandle } from 'react-virtuoso'
import { favoriteClipboardItem, Filter, unfavoriteClipboardItem } from '@/api/clipboardItems'
import { toast } from '@/components/ui/toast'
import { useCopyFeedback } from '@/hooks/useCopyFeedback'
import { useDeleteFlow } from '@/hooks/useDeleteFlow'
import { useHistoryData } from '@/hooks/useHistoryData'
import { useSearchTags } from '@/hooks/useSearchTags'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { useTransferProgress } from '@/hooks/useTransferProgress'
import { useAppDispatch } from '@/store/hooks'
import { copyToClipboard, removeClipboardItem } from '@/store/slices/clipboardSlice'
import {
  readHistorySessionSnapshot,
  updateHistorySessionSelection,
  writeHistorySessionSnapshot,
} from './historySessionSnapshot'

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

  const data = useHistoryData()
  const searchableTags = useSearchTags()
  const initialSnapshot = readHistorySessionSnapshot()

  // Per-card interaction state (small + render-driving).
  const [hoveredId, setHoveredId] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(
    () => initialSnapshot?.selectedId ?? null
  )
  // Stable Set of ids already rendered once; read during render to gate the
  // entrance animation, mutated only in an effect (never during render).
  const [seenIds] = useState(() => new Set<string>(initialSnapshot?.seenIds ?? []))
  const [scrollState, setScrollState] = useState<StateSnapshot | null>(
    () => initialSnapshot?.scrollState ?? null
  )
  const listRef = useRef<VirtuosoHandle>(null)
  const latestSnapshotRef = useRef({
    searchState: data.filter,
    live: data.liveSnapshot,
    selectedId,
  })
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

  const selectedItem = useMemo(
    () => orderedItems.find(it => it.id === selectedId) ?? null,
    [orderedItems, selectedId]
  )

  // Keep hover/selection valid and the preview pane populated:
  // - drop a hover/selection that points at an entry no longer in the list
  //   (after a delete, filter switch, or search change)
  // - auto-select the first row when nothing is selected but items exist, so the
  //   master-detail preview is never blank while there is something to show.
  useEffect(() => {
    const ids = new Set(orderedItems.map(it => it.id))
    if (hoveredId !== null && !ids.has(hoveredId)) setHoveredId(null)
    if (selectedId !== null && !ids.has(selectedId)) {
      setSelectedId(null)
      return
    }
    if (selectedId === null && orderedItems.length > 0) {
      setSelectedId(orderedItems[0].id)
    }
  }, [orderedItems, hoveredId, selectedId])

  const handleCardClick = useCallback((id: string) => setSelectedId(id), [])

  useEffect(() => {
    if (selectedId !== null) updateHistorySessionSelection(selectedId)
  }, [selectedId])

  useEffect(() => {
    latestSnapshotRef.current = {
      searchState: data.filter,
      live: data.liveSnapshot,
      selectedId,
    }
  }, [data.filter, data.liveSnapshot, selectedId])

  useEffect(() => {
    return () => {
      listRef.current?.getState(nextScrollState => {
        const latest = latestSnapshotRef.current
        writeHistorySessionSnapshot({
          searchState: latest.searchState,
          live: latest.live,
          selectedId: latest.selectedId,
          seenIds: Array.from(seenIds),
          scrollState: nextScrollState,
        })
      })
    }
  }, [seenIds])

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
    requestDelete,
    handleToggleFavorite,
    handleCardClick,

    // Delete confirmation dialog.
    deleteDialogOpen,
    setDeleteDialogOpen,
    confirmDelete,
  }
}
