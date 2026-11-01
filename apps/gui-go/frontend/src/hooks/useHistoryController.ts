import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { StateSnapshot, VirtuosoHandle } from 'react-virtuoso'
import { favoriteClipboardItem, Filter, unfavoriteClipboardItem } from '@/api/clipboardItems'
import { restoreClipboardEntry } from '@/api/daemon'
import {
  addTagToEntries,
  createHistoryTag,
  deleteHistoryTag,
  mergeHistoryTags,
  removeTagFromEntries,
  renameHistoryTag,
  setHistoryTagColor,
  setHistoryTagInSidebar,
  type HistoryTagColorDto,
  type HistoryTagRenameResultDto,
} from '@/api/daemon/history-tags'
import { toast } from '@/components/ui/toast'
import { useCopyFeedback } from '@/hooks/useCopyFeedback'
import { useDeleteFlow } from '@/hooks/useDeleteFlow'
import { useHistoryData } from '@/hooks/useHistoryData'
import { useOutboundProgress } from '@/hooks/useOutboundProgress'
import { useSelectionTagSummary } from '@/hooks/useSelectionTagSummary'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { useTagCatalog } from '@/hooks/useTagCatalog'
import { useTransferProgress } from '@/hooks/useTransferProgress'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { createLogger } from '@/lib/logger'
import { splitSearchTags, tagLabel } from '@/lib/search-tags'
import { DEFAULT_TAG_COLOR } from '@/lib/tag-colors'
import { useAppDispatch } from '@/store/hooks'
import { copyToClipboard, removeClipboardItem } from '@/store/slices/clipboardSlice'
import { fetchLocalDeviceInfo, fetchSpaceMembers } from '@/store/slices/devicesSlice'
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
  useOutboundProgress()

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
    // Most rows originate on this device; without its name their meta line
    // would fall back to a truncated device id.
    dispatch(fetchLocalDeviceInfo())
      .unwrap()
      .catch(err => {
        log.warn({ err }, 'failed to load local device info')
      })
  }, [dispatch])

  const initialSnapshot = readHistorySessionSnapshot()
  const [selection, setSelection] = useState(() => ({
    id: initialSnapshot?.selectedId ?? null,
    item: initialSnapshot?.selectedItem ?? (null as DisplayClipboardItem | null),
    index: initialSnapshot?.selectedIndex ?? (null as number | null),
    items: [] as DisplayClipboardItem[],
    offset: initialSnapshot?.live?.offset ?? 0,
    key: JSON.stringify(initialSnapshot?.live?.model ?? null),
  }))
  const [checkedState, setCheckedState] = useState<ReadonlyMap<string, boolean>>(() => new Map())
  const data = useHistoryData(selection.id)
  // One revision drives both tag sources: bump it after any tag change.
  const [tagRevision, setTagRevision] = useState(0)
  const { searchableTags, historyTags, layout: tagLayout } = useTagCatalog(tagRevision)
  const { refetch } = data
  // After a tag change: names and counts, and the rows' own tag ids.
  const refreshTags = useCallback(() => {
    setTagRevision(revision => revision + 1)
    refetch()
  }, [refetch])
  // Hover only selects a keyboard shortcut target; visual state belongs to the card.
  const hoveredIdRef = useRef<string | null>(null)
  const handleHoverChange = useCallback((id: string, hovered: boolean) => {
    if (hovered) hoveredIdRef.current = id
    else if (hoveredIdRef.current === id) hoveredIdRef.current = null
  }, [])
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

  const { copySuccessId, markCopied } = useCopyFeedback()
  // The engine moves a restored entry to the result head. Re-read the current
  // window in place rather than scrolling the reader away; the copied entry
  // keeps its selected projection until it is loaded again at its new rank.
  const refreshAfterCopy = useCallback(
    (id: string) => {
      setSelection(previous => (previous.id === id ? { ...previous, index: null } : previous))
      refetch()
    },
    [refetch]
  )

  // ── Copy handler ──────────────────────────────────────────────
  const handleCopy = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await dispatch(copyToClipboard(id)).unwrap()
        markCopied(id)
        refreshAfterCopy(id)
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
    [dispatch, t, markCopied, refreshAfterCopy]
  )

  const handleCopyFilePaths = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await restoreClipboardEntry(id, { filePathsOnly: true })
        markCopied(id)
        refreshAfterCopy(id)
        return true
      } catch {
        toast.error(t('clipboard.errors.copyFailed'))
        return false
      }
    },
    [markCopied, t, refreshAfterCopy]
  )

  // ── Delete flow ───────────────────────────────────────────────
  const removeEntry = useCallback(
    async (id: string) => {
      try {
        await dispatch(removeClipboardItem(id)).unwrap()
        setSelection(previous =>
          previous.id === id ? { ...previous, id: null, item: null, index: null } : previous
        )
        setCheckedState(previous => {
          const next = new Map(previous)
          next.delete(id)
          return next
        })
        data.removeItem(id)
        data.refetch()
      } catch {
        toast.error(t('clipboard.errors.deleteFailed', 'Delete failed'))
      }
    },
    [dispatch, t, data.removeItem, data.refetch]
  )

  const {
    deleteDialogOpen,
    setDeleteDialogOpen,
    deletingIds,
    deleteCount,
    requestDelete,
    confirmDelete,
  } = useDeleteFlow(removeEntry)

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

  // Ordering belongs to the engine. A copy refreshes the head rather than
  // promoting a row inside a deep window and corrupting its absolute index.
  const orderedItems = useMemo(() => {
    const hasOverrides = Object.keys(favoriteOverrides).length > 0
    return hasOverrides
      ? data.baseItems.map(it =>
          it.id in favoriteOverrides ? { ...it, isFavorited: favoriteOverrides[it.id] } : it
        )
      : data.baseItems
  }, [data.baseItems, favoriteOverrides])
  const offset = data.viewport.offset
  const resultKey = data.resultKey
  const resultsReady = data.resultsReady
  const queryChanged = selection.key !== resultKey

  if (selection.items !== orderedItems || (queryChanged && resultsReady)) {
    const ids = new Set(orderedItems.map(item => item.id))
    let id = selection.id
    let index = selection.index
    const previousIndex = selection.items.findIndex(item => item.id === id)
    const sharedIndex = orderedItems.findIndex(item =>
      selection.items.some(old => old.id === item.id)
    )
    if (sharedIndex >= 0 && index !== null) {
      const oldIndex = selection.items.findIndex(item => item.id === orderedItems[sharedIndex].id)
      index += offset + sharedIndex - selection.offset - oldIndex
    }
    const inLoadedRange = index !== null && index >= offset && index < offset + orderedItems.length
    if (
      resultsReady &&
      id !== null &&
      !ids.has(id) &&
      (queryChanged || inLoadedRange || data.browseCount === 0)
    )
      id = null
    const first = orderedItems[0]?.id ?? null
    if (
      !queryChanged &&
      data.isAtTop() &&
      offset === 0 &&
      selection.offset === 0 &&
      previousIndex === 0 &&
      selection.index === 0 &&
      id === selection.items[0]?.id &&
      first &&
      first !== id &&
      !selection.items.some(item => item.id === first)
    )
      id = first
    if (id === null && first !== null && resultsReady) id = first
    const loadedIndex = orderedItems.findIndex(item => item.id === id)
    const item =
      loadedIndex >= 0 ? orderedItems[loadedIndex] : id === selection.id ? selection.item : null
    if (loadedIndex >= 0) index = offset + loadedIndex
    if (id === null) index = null
    setSelection({
      id,
      item,
      index,
      items: orderedItems,
      offset,
      key: resultsReady ? resultKey : selection.key,
    })
    if (queryChanged && resultsReady) {
      setCheckedState(previous => new Map([...previous].filter(([checked]) => ids.has(checked))))
    }
  }
  const selectedId = selection.id
  const selectedItem = useMemo(
    () =>
      selection.item
        ? selection.id && selection.id in favoriteOverrides
          ? { ...selection.item, isFavorited: favoriteOverrides[selection.id] }
          : selection.item
        : null,
    [selection.item, selection.id, favoriteOverrides]
  )
  const orderedItemIds = useMemo(() => new Set(orderedItems.map(item => item.id)), [orderedItems])

  // Bulk selection owns identities, not off-window record projections.
  const checkedIds = useMemo(() => new Set(checkedState.keys()), [checkedState])
  const toggleChecked = useCallback(
    (id: string) => {
      setCheckedState(prev => {
        const next = new Map(prev)
        if (!next.delete(id))
          next.set(id, orderedItems.find(item => item.id === id)?.isFavorited === true)
        return next
      })
    },
    [orderedItems]
  )
  const clearChecked = useCallback(() => setCheckedState(new Map()), [])
  /** Keep only identities and pin flags for off-window bulk actions. */
  const setChecked = useCallback(
    (ids: readonly string[]) =>
      setCheckedState(
        previous =>
          new Map(
            ids.map(id => [
              id,
              orderedItems.find(item => item.id === id)?.isFavorited ?? previous.get(id) ?? false,
            ])
          )
      ),
    [orderedItems]
  )
  const checkedItems = useMemo(
    () => orderedItems.filter(item => checkedIds.has(item.id)),
    [orderedItems, checkedIds]
  )
  const checkedPins = new Map(
    [...checkedState].map(([id, pinned]) => [
      id,
      favoriteOverrides[id] ?? orderedItems.find(item => item.id === id)?.isFavorited ?? pinned,
    ])
  )
  const checkedAllPinned = checkedPins.size > 0 && [...checkedPins.values()].every(Boolean)
  const pinChecked = () => {
    for (const [id, current] of checkedPins) {
      if (current === checkedAllPinned) void handleToggleFavorite(id, current)
    }
  }
  const deleteChecked = useCallback(
    () => requestDelete([...checkedIds]),
    [checkedIds, requestDelete]
  )

  useShortcut({
    key: 'escape',
    scope: 'clipboard',
    enabled: checkedIds.size > 0,
    handler: clearChecked,
  })

  // ── Hover keyboard shortcuts ──────────────────────────────────
  useShortcut({
    key: 'c',
    scope: 'clipboard',
    handler: () => {
      const id = hoveredIdRef.current
      if (id !== null && orderedItemIds.has(id)) handleCopy(id)
    },
    preventDefault: false,
  })

  useShortcut({
    key: 'd',
    scope: 'clipboard',
    handler: () => {
      const id = hoveredIdRef.current
      if (id !== null && orderedItemIds.has(id)) requestDelete(id)
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

  // ── Local tags ─────────────────────────────────────────────────
  // The entry whose tag editor is open; switching entries closes it.
  const [tagEditorFor, setTagEditorFor] = useState<string | null>(null)
  // With several rows checked, the detail column tags the whole selection
  // (HDetail.dc.html `multi`); its editor closes once fewer than two remain.
  const selectionTagging = checkedIds.size > 1
  const [selectionEditorWanted, setSelectionTagEditorOpen] = useState(false)
  const selectionTagEditorOpen = selectionEditorWanted && selectionTagging
  const checkedIdList = useMemo(() => [...checkedIds], [checkedIds])
  // Local tag id → name (`null`: unreadable), for the list rows' chips.
  const tagNames = useMemo(
    () => new Map(historyTags.tags.map(tag => [tag.tagId, tag.name ?? null])),
    [historyTags.tags]
  )
  const selectionTagSummary = useSelectionTagSummary(
    checkedIdList,
    historyTags.available && selectionTagging,
    tagRevision
  )
  useShortcut({
    key: 't',
    scope: 'clipboard',
    enabled: (selectionTagging || selectedItem !== null) && historyTags.available,
    handler: () => {
      if (selectionTagging) setSelectionTagEditorOpen(true)
      else setTagEditorFor(selectedItem?.id ?? null)
    },
  })

  // Every tag change ends the same way: refetch names, counts and rows, or say
  // it failed. Returns whether it succeeded.
  const runTagChange = useCallback(
    async (change: () => Promise<unknown>): Promise<boolean> => {
      try {
        await change()
        return true
      } catch (err) {
        log.warn({ err }, 'history tag change failed')
        toast.error(t('history.tags.failed'))
        return false
      } finally {
        refreshTags()
      }
    },
    [refreshTags, t]
  )
  const addTagToItems = useCallback(
    (tagId: string, entryIds: string[]) => runTagChange(() => addTagToEntries(tagId, entryIds)),
    [runTagChange]
  )
  const removeTagFromItems = useCallback(
    (tagId: string, entryIds: string[]) =>
      runTagChange(() => removeTagFromEntries(tagId, entryIds)),
    [runTagChange]
  )
  // The Library's tag manager (HManage.dc.html, tags tab).
  const [tagManagerOpen, setTagManagerOpen] = useState(false)
  const createTag = useCallback(
    (name: string, color?: HistoryTagColorDto) => runTagChange(() => createHistoryTag(name, color)),
    [runTagChange]
  )
  /** A tag's color in the tag layout; `null` clears it. */
  const setTagColor = useCallback(
    (tagId: string, color: HistoryTagColorDto | null) =>
      runTagChange(() => setHistoryTagColor(tagId, color)),
    [runTagChange]
  )
  /** Show a tag in the sidebar (last) or take it out. */
  const setTagInSidebar = useCallback(
    (tagId: string, inSidebar: boolean) =>
      runTagChange(() => setHistoryTagInSidebar(tagId, inSidebar)),
    [runTagChange]
  )
  /** Resolves to the outcome (`renamed` or `name_conflict`), or `null` on failure. */
  const renameTag = useCallback(
    async (tagId: string, name: string): Promise<HistoryTagRenameResultDto | null> => {
      let result: HistoryTagRenameResultDto | null = null
      await runTagChange(async () => {
        result = await renameHistoryTag(tagId, name)
      })
      return result
    },
    [runTagChange]
  )
  const mergeTagsInto = useCallback(
    (targetTagId: string, sourceTagIds: string[]) =>
      runTagChange(() => mergeHistoryTags(targetTagId, sourceTagIds)),
    [runTagChange]
  )
  /** Delete tags one after another; their items stay. */
  const deleteTags = useCallback(
    (tagIds: string[]) =>
      runTagChange(async () => {
        for (const tagId of tagIds) await deleteHistoryTag(tagId)
      }),
    [runTagChange]
  )

  /** Create-or-get by name for the search field's "+ Create #name"; resolves
   * to the tag's id, or `null` when it failed. */
  const createTagForSearch = useCallback(
    async (name: string): Promise<string | null> => {
      let tagId: string | null = null
      await runTagChange(async () => {
        tagId = (await createHistoryTag(name, DEFAULT_TAG_COLOR)).tag.tagId
      })
      return tagId
    },
    [runTagChange]
  )

  /** Create-or-get by name (the Engine folds case and spacing), then attach. */
  const tagItemsByName = useCallback(
    (name: string, entryIds: string[], color?: HistoryTagColorDto) =>
      runTagChange(async () => {
        const { tag } = await createHistoryTag(name, color)
        await addTagToEntries(tag.tagId, entryIds)
      }),
    [runTagChange]
  )

  const navigation = useRef<{ index: number; delta: number; token: symbol } | null>(null)
  const handleCardClick = useCallback(
    (id: string) => {
      navigation.current = null
      const index = orderedItems.findIndex(item => item.id === id)
      if (index < 0) return
      setSelection({
        id,
        item: orderedItems[index],
        index: offset + index,
        items: orderedItems,
        offset,
        key: resultKey,
      })
    },
    [orderedItems, offset, resultKey]
  )

  useEffect(() => {
    updateHistorySessionSelection(selectedId, selectedItem, selection.index)
  }, [selectedId, selectedItem, selection.index])

  const writeCurrentSnapshot = useEffectEvent((nextScrollState: StateSnapshot) => {
    writeHistorySessionSnapshot({
      searchState: data.filter,
      live: data.liveSnapshot,
      selectedId,
      selectedItem,
      selectedIndex: selection.index,
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

  const moveSelection = useCallback(
    async (movement: number | 'first' | 'last') => {
      const start = navigation.current?.index ?? selection.index ?? offset
      const target = Math.max(
        0,
        Math.min(
          Math.max(0, data.viewport.total - 1),
          movement === 'first'
            ? 0
            : movement === 'last'
              ? data.viewport.total - 1
              : start + movement
        )
      )
      const token = Symbol()
      const delta = typeof movement === 'number' ? (navigation.current?.delta ?? 0) + movement : 0
      navigation.current = { index: target, delta, token }
      let resolved: { item: DisplayClipboardItem; index: number } | null
      if (
        typeof movement === 'number' &&
        selection.id !== null &&
        !orderedItemIds.has(selection.id)
      ) {
        resolved = await data.ensureNeighbor(selection.id, selection.index ?? 0, delta)
      } else {
        const item = await data.ensureItem(target)
        resolved = item ? { item, index: target } : null
      }
      if (navigation.current?.token !== token) return null
      navigation.current = null
      if (!resolved) return null
      const { item, index } = resolved
      setSelection(previous => ({ ...previous, id: item.id, item, index }))
      return item.id
    },
    [
      data.ensureItem,
      data.ensureNeighbor,
      data.viewport.total,
      selection.id,
      selection.index,
      orderedItemIds,
      offset,
    ]
  )

  // Heading reflects the active quick view / content-type filter ("收藏",
  // "文本", …), falling back to "全部" while unfiltered.
  const viewLabel =
    data.filter.tagFilter !== null
      ? splitSearchTags(data.filter.tagFilter)
          .map(id => tagLabel(searchableTags.find(tag => tag.id === id) ?? { id }, t))
          .join(', ')
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
    sourceDeviceNames: data.sourceDeviceNames,
    searchableTags,
    historyTags,
    tagLayout,
    tagNames,
    refreshTags,
    tagEditorFor,
    setTagEditorFor,
    selectionTagSummary,
    selectionTagEditorOpen,
    setSelectionTagEditorOpen,
    addTagToItems,
    removeTagFromItems,
    tagItemsByName,
    tagManagerOpen,
    setTagManagerOpen,
    createTag,
    setTagColor,
    setTagInSidebar,
    createTagForSearch,
    renameTag,
    mergeTagsInto,
    deleteTags,
    browseCount: data.browseCount,
    queryFailure: data.queryFailure,
    retryQuery: data.refetch,
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
    viewport: { ...data.viewport, resetKey: resultKey, onMoveSelection: moveSelection },
    handleHoverChange,
    selectedId,
    copySuccessId,
    deletingIds,

    // Bulk selection.
    checkedIds,
    checkedItems,
    checkedAllPinned,
    toggleChecked,
    clearChecked,
    setChecked,
    pinChecked,
    deleteChecked,

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
    deleteCount,
    confirmDelete,
  }
}
