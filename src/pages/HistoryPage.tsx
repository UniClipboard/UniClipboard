import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import { CompositeSearchBar, FilterBar } from '@/components/history/composite-search'
import HistoryDetailSheet from '@/components/history/HistoryDetailSheet'
import HistoryGrid from '@/components/history/HistoryGrid'
import { toast } from '@/components/ui/toast'
import { useCopyFeedback } from '@/hooks/useCopyFeedback'
import { useDeleteFlow } from '@/hooks/useDeleteFlow'
import { useHistoryData } from '@/hooks/useHistoryData'
import { useHistoryInfiniteScroll } from '@/hooks/useHistoryInfiniteScroll'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { useTransferProgress } from '@/hooks/useTransferProgress'
import { useAppDispatch } from '@/store/hooks'
import { copyToClipboard, removeClipboardItem } from '@/store/slices/clipboardSlice'

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()

  useShortcutScope('clipboard')
  // Activate the file-transfer progress event listener for this page.
  useTransferProgress()

  const data = useHistoryData()

  // Per-card interaction state kept on the page (small + render-driving).
  const [hoveredId, setHoveredId] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  // Stable Set of ids already rendered once; read during render to gate the
  // entrance animation, mutated only in an effect (never during render).
  const [seenIds] = useState(() => new Set<string>())
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

  // ── Delete handlers ───────────────────────────────────────────
  // Grid delete goes through a confirm dialog + exit animation (useDeleteFlow);
  // the detail sheet deletes directly and needs a boolean to close itself.
  const removeEntry = useCallback(
    async (id: string) => {
      try {
        await dispatch(removeClipboardItem(id)).unwrap()
      } catch {
        toast.error(t('clipboard.errors.deleteFailed', 'Delete failed'))
      }
    },
    [dispatch, t]
  )

  const { deleteDialogOpen, setDeleteDialogOpen, deletingId, requestDelete, confirmDelete } =
    useDeleteFlow(removeEntry)

  const handleSheetDelete = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await dispatch(removeClipboardItem(id)).unwrap()
        return true
      } catch {
        toast.error(t('clipboard.errors.deleteFailed', 'Delete failed'))
        return false
      }
    },
    [dispatch, t]
  )

  // Float the just-copied entry to the front of the list.
  const orderedItems = useMemo(() => {
    const base = data.baseItems
    if (!promotedId) return base
    const idx = base.findIndex(it => it.id === promotedId)
    if (idx <= 0) return base
    return [base[idx], ...base.slice(0, idx), ...base.slice(idx + 1)]
  }, [data.baseItems, promotedId])

  const scrollRef = useHistoryInfiniteScroll({
    isSearchActive: data.isSearchActive,
    hasMore: data.hasMore,
    handleLoadMore: data.handleLoadMore,
    searchTotal: data.searchTotal,
    searchLoadedCount: data.searchLoadedCount,
    searchLoading: data.searchLoading,
    growSearchWindow: data.growSearchWindow,
    items: orderedItems,
  })

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

  // Record rendered ids after commit so subsequent remounts (e.g. column shifts
  // when a new item is prepended) skip the entrance animation.
  useEffect(() => {
    for (const item of orderedItems) seenIds.add(item.id)
  }, [orderedItems, seenIds])

  const selectedItem = useMemo(
    () => orderedItems.find(it => it.id === selectedId) ?? null,
    [orderedItems, selectedId]
  )

  // Drop hover/selection that point at entries no longer in the list (after a
  // delete, filter switch, or search change) so the keyboard shortcuts and the
  // detail sheet never act on — or render — a missing id.
  useEffect(() => {
    const ids = new Set(orderedItems.map(it => it.id))
    if (hoveredId !== null && !ids.has(hoveredId)) setHoveredId(null)
    if (selectedId !== null && !ids.has(selectedId)) setSelectedId(null)
  }, [orderedItems, hoveredId, selectedId])

  const handleCardClick = useCallback((id: string) => setSelectedId(id), [])

  return (
    <div className="flex flex-col h-full">
      {/* ── Toolbar: quick filters (left) + composite search (right) ─ */}
      <div className="shrink-0 flex flex-wrap items-center gap-x-3 gap-y-2 px-2 pt-3 pb-2">
        <FilterBar
          contentFilter={data.filter.activeFilter}
          sourceFilter={data.filter.sourceFilter}
          timeRange={data.filter.timeRange}
          onContentFilterChange={data.actions.setContentFilter}
          onSourceFilterChange={data.actions.setSourceFilter}
          onTimeRangeChange={data.actions.setTimeRange}
          sourceOptions={data.sourceOptions}
        />
        <div className="ml-auto w-80 max-w-full">
          <CompositeSearchBar
            contentFilter={data.filter.activeFilter}
            sourceFilter={data.filter.sourceFilter}
            timeRange={data.filter.timeRange}
            onContentFilterChange={data.actions.setContentFilter}
            onSourceFilterChange={data.actions.setSourceFilter}
            onTimeRangeChange={data.actions.setTimeRange}
            onQueryChange={data.actions.setQuery}
            onQuerySubmit={text => data.actions.submitQuery(text.trim())}
            sourceOptions={data.sourceOptions}
            totalCount={data.browseCount}
            inputRef={searchInputRef}
          />
        </div>
      </div>

      {/* ── Grid ───────────────────────────────────────────────── */}
      <HistoryGrid
        scrollRef={scrollRef}
        items={orderedItems}
        seenIds={seenIds}
        isSearchActive={data.isSearchActive}
        submittedQuery={data.filter.submittedQuery}
        searchLoading={data.searchLoading}
        hoveredId={hoveredId}
        copySuccessId={copySuccessId}
        deletingId={deletingId}
        onCopy={handleCopy}
        onCardClick={handleCardClick}
        onHoverChange={setHoveredId}
      />

      <HistoryDetailSheet
        item={selectedItem}
        open={selectedItem !== null}
        onOpenChange={open => {
          if (!open) setSelectedId(null)
        }}
        onCopy={handleCopy}
        onDelete={handleSheetDelete}
      />

      <DeleteConfirmDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        onConfirm={confirmDelete}
        count={1}
      />
    </div>
  )
}

export default HistoryPage
