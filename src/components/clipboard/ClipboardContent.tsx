import { Inbox, Loader2, Search } from 'lucide-react'
import React, { useMemo, useState, useEffect, useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useDefaultLayout } from 'react-resizable-panels'
import { GroupedVirtuoso, type GroupedVirtuosoHandle } from 'react-virtuoso'
import { Filter, copyFileToClipboard, openFileLocation } from '@/api/clipboardItems'
import { querySearch } from '@/api/daemon/search'
import type { SearchResultDto } from '@/api/daemon/search'
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable'
import { toast } from '@/components/ui/toast'
import { useFileSyncNotifications } from '@/hooks/useFileSyncNotifications'
import { usePlatform } from '@/hooks/usePlatform'
import { useShortcut } from '@/hooks/useShortcut'
import { useTransferProgress } from '@/hooks/useTransferProgress'
import type { ClipboardFileItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { firstRevealableFilePath } from '@/lib/clipboard-utils'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'
import { captureUserIntent } from '@/observability/breadcrumbs'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import {
  removeClipboardItem,
  copyToClipboard,
  markEntryStale,
  type PendingClipboardEntry,
} from '@/store/slices/clipboardSlice'
import { selectEntryTransferStatus } from '@/store/slices/fileTransferSlice'
import ClipboardActionBar from './ClipboardActionBar'
import ClipboardListRow from './ClipboardListRow'
import ClipboardPreview from './ClipboardPreview'
import DeleteConfirmDialog from './DeleteConfirmDialog'

const log = createLogger('clipboard-content')

interface DateGroup {
  label: string
  items: DisplayClipboardItem[]
}

interface ClipboardContentProps {
  filter: Filter
  searchQuery?: string
  timeRange?: import('@/contexts/search-context').TimeRangePreset
  hasMore?: boolean
  onLoadMore?: () => void
}

function groupItemsByDate(items: DisplayClipboardItem[], t: (key: string) => string): DateGroup[] {
  if (items.length === 0) return []

  const now = new Date()
  const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  const yesterdayStart = todayStart - 86400000

  const today: DisplayClipboardItem[] = []
  const yesterday: DisplayClipboardItem[] = []
  const earlier: DisplayClipboardItem[] = []

  for (const item of items) {
    if (item.activeTime >= todayStart) {
      today.push(item)
    } else if (item.activeTime >= yesterdayStart) {
      yesterday.push(item)
    } else {
      earlier.push(item)
    }
  }

  const groups: DateGroup[] = []
  if (today.length > 0) groups.push({ label: t('clipboard.dateGroup.today'), items: today })
  if (yesterday.length > 0)
    groups.push({ label: t('clipboard.dateGroup.yesterday'), items: yesterday })
  if (earlier.length > 0) groups.push({ label: t('clipboard.dateGroup.earlier'), items: earlier })
  return groups
}

/** Map backend contentType to frontend display type. */
function mapSearchContentType(ft: SearchResultDto['contentType']): DisplayClipboardItem['type'] {
  switch (ft) {
    case 'text':
      return 'text'
    case 'html':
      return 'code'
    case 'link':
      return 'link'
    case 'file':
      return 'file'
    case 'image':
      return 'image'
    case 'other':
      return 'unknown'
  }
}

/** Compact byte formatter used only for placeholder card hint text. */
function formatBytesShort(bytes: number): string {
  if (bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const k = 1024
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), units.length - 1)
  const value = bytes / Math.pow(k, i)
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`
}

/**
 * Pick the placeholder hint text for a pending inbound entry. Used as the
 * row preview when the placeholder has no filenames (text-only / pure
 * image inbound) — when filenames exist we let the normal `file` row
 * preview path render the filename list, matching the eventual real entry.
 */
function buildPendingPreview(
  entry: PendingClipboardEntry,
  t: (key: string, opts?: Record<string, unknown>) => string
): string {
  if (entry.totalBytes != null && entry.totalBytes > 0) {
    return t('clipboard.transfer.incomingWithSize', {
      size: formatBytesShort(entry.totalBytes),
    })
  }
  return t('clipboard.transfer.incoming')
}

/**
 * Build a minimal `ClipboardFileItem` for a pending inbound entry so the
 * right-side `FilePreview` can render the filename + progress overlay
 * during the fetch window (otherwise it short-circuits on null content
 * and the panel goes blank until the real entry lands).
 *
 * Per-file sizes aren't known from `incoming_pending` (only the envelope
 * total is). For single-file inbounds we map total → that one file's size;
 * for multi-file we report `-1` per slot, which `FilePreview` already
 * understands as "size unknown, hide the byte count".
 */
function buildPendingFileContent(entry: PendingClipboardEntry): ClipboardFileItem | null {
  if (entry.filenames.length === 0) return null
  const fileSizes: number[] =
    entry.filenames.length === 1 && entry.totalBytes != null && entry.totalBytes > 0
      ? [entry.totalBytes]
      : entry.filenames.map(() => -1)
  return { file_names: entry.filenames, file_sizes: fileSizes }
}

const ClipboardContent: React.FC<ClipboardContentProps> = ({
  filter,
  searchQuery = '',
  timeRange = 'all_time',
  hasMore = true,
  onLoadMore,
}) => {
  const { t } = useTranslation()
  const { isWindows } = usePlatform()

  // Activate transfer progress event listener
  useTransferProgress()
  // Activate file sync notification batching
  useFileSyncNotifications()

  const dispatch = useAppDispatch()

  // Persist panel layout to localStorage
  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: 'clipboard-panels',
    panelIds: ['clipboard-list', 'clipboard-preview'],
    storage: localStorage,
  })
  const {
    items: reduxItems,
    pendingItems,
    loading,
    notReady,
    staleEntryIds,
  } = useAppSelector(state => state.clipboard)
  const spaceMembers = useAppSelector(state => state.devices.spaceMembers)
  // peerId → human-readable device name. Used to translate the raw
  // DeviceId on `incoming_pending` events into the same display string
  // used elsewhere; falls back to undefined when the peer isn't in our
  // member roster (e.g. roster not yet loaded), so we hide the field
  // rather than leak the UUID into the file preview.
  const deviceNameByPeerId = useMemo(() => {
    const map: Record<string, string> = {}
    for (const m of spaceMembers) map[m.peerId] = m.deviceName
    return map
  }, [spaceMembers])

  // Server-side search state
  const isSearchActive = searchQuery.trim().length > 0
  const [searchResults, setSearchResults] = useState<DisplayClipboardItem[] | null>(null)
  const [searchLoading, setSearchLoading] = useState(false)
  const [searchTotal, setSearchTotal] = useState<number | null>(null)
  const searchAbortRef = useRef<AbortController | null>(null)

  // Server-side search effect
  useEffect(() => {
    if (!isSearchActive) {
      setSearchResults(null)
      setSearchTotal(null)
      setSearchLoading(false)
      return
    }

    searchAbortRef.current?.abort()
    const controller = new AbortController()
    searchAbortRef.current = controller

    setSearchLoading(true)

    // Filter/timeRange values match backend params directly;
    // Code includes html (html is a form of code)
    let contentTypes: string | undefined
    if (filter === Filter.Code) contentTypes = 'code,html'
    else if (filter !== Filter.All && filter !== Filter.Favorited) contentTypes = filter
    const timePreset = timeRange !== 'all_time' ? timeRange : undefined

    querySearch(
      {
        query: searchQuery,
        contentTypes,
        timePreset,
        limit: 50,
      },
      controller.signal
    )
      .then(response => {
        if (controller.signal.aborted) return
        // ADR-008 §0.1: items + total now live inside the enveloped `data` payload.
        const items: DisplayClipboardItem[] = response.data.items.map(r => ({
          id: r.entryId,
          type: mapSearchContentType(r.contentType),
          activeTime: r.activeTimeMs,
          content: null,
          textPreview: r.textPreview ?? undefined,
        }))
        setSearchResults(items)
        setSearchTotal(response.data.total)
        setSearchLoading(false)
      })
      .catch(err => {
        if (controller.signal.aborted) return
        if (err instanceof DOMException && err.name === 'AbortError') return
        log.error({ err }, 'Dashboard search failed')
        setSearchResults([])
        setSearchTotal(0)
        setSearchLoading(false)
      })

    return () => {
      controller.abort()
    }
  }, [searchQuery, filter, timeRange, isSearchActive, t])

  const [activeItemId, setActiveItemId] = useState<string | null>(null)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [copySuccess, setCopySuccess] = useState(false)

  const virtuosoRef = useRef<GroupedVirtuosoHandle>(null)
  // 用户的视觉锚是否还贴在列表顶部。初次进入、点击/键盘把 active 放到第一项、
  // auto-follow 跟到新顶 都会把它设为 true;一旦用户主动选了非第一项就转 false。
  // 用 ref 跟踪而不是对比上一帧 first id, 是因为 effect 还会被 filter 切换、
  // reduxItems 引用变化等非用户事件触发, 那些不该改变锚。
  const anchoredToTopRef = useRef(true)

  // Build display items: server search results or Redux browse items
  const clipboardItems = useMemo(() => {
    // When a search query is active, use server-side results
    if (isSearchActive && searchResults !== null) {
      return searchResults
    }

    // Browse mode: render the domain entries directly. The relative-time label
    // is no longer baked in here (each row derives it from `activeTime`), so
    // entry identity stays stable across clock ticks — no per-tick rebuild.
    const realItems: DisplayClipboardItem[] = reduxItems ?? []

    // Pending placeholder rows (inbound entries that have been announced but
    // not yet fetched + persisted). We surface them as 'file' so the
    // existing transferring/pending visuals in ClipboardItemRow apply.
    // Filter by entryId so once the real entry lands we don't double-count.
    // `textPreview` is built with a 3-tier fallback so the row never
    // renders blank:
    //   1. ≥1 filename advertised in the V3 envelope → show first name
    //      (`+N` suffix when multiple), exactly the same shape as the
    //      eventual real `ClipboardFileItem` row;
    //   2. no filenames but a known total size → "Receiving (3.2 MB)…";
    //   3. neither → generic "Receiving…" fallback (e.g. pure image blob).
    const realIds = new Set(realItems.map(it => it.id))
    const pendingDisplayItems: DisplayClipboardItem[] = pendingItems.flatMap(p =>
      realIds.has(p.entryId)
        ? []
        : [
            {
              id: p.entryId,
              type: 'file' as const,
              activeTime: p.createdAt,
              // Synthesize a ClipboardFileItem from the V3-advertised filenames so
              // FilePreview renders the file card + progress overlay immediately,
              // not just after fetch completes. Falls back to null only when the
              // inbound has no filenames at all (pure image / text), in which case
              // textPreview carries the "Receiving..." fallback.
              content: buildPendingFileContent(p),
              // Resolve raw peerId → device name; if the roster doesn't know
              // this peer yet, leave undefined so FilePreview hides the field
              // instead of rendering a UUID next to the file size.
              device: deviceNameByPeerId[p.fromDevice],
              textPreview: buildPendingPreview(p, t),
            },
          ]
    )

    let items = [...pendingDisplayItems, ...realItems]

    if (filter !== Filter.All) {
      if (filter === Filter.Favorited) {
        items = items.filter(it => it.isFavorited)
      } else {
        const filterTypeMap: Record<string, string> = {
          [Filter.Text]: 'text',
          [Filter.Image]: 'image',
          [Filter.Link]: 'link',
          [Filter.File]: 'file',
          [Filter.Code]: 'code',
        }
        const targetType = filterTypeMap[filter]
        if (targetType) {
          items = items.filter(it => it.type === targetType)
        }
      }
    }

    return items
  }, [reduxItems, pendingItems, deviceNameByPeerId, filter, isSearchActive, searchResults, t])

  // Date groups for rendering
  const dateGroups = useMemo(() => groupItemsByDate(clipboardItems, t), [clipboardItems, t])

  // Flat list for keyboard navigation + virtualization. Derived from the
  // groups (not raw clipboardItems) so a flat index maps 1:1 to the item the
  // GroupedVirtuoso renders at that position.
  const flatItems = useMemo(() => dateGroups.flatMap(g => g.items), [dateGroups])
  const groupCounts = useMemo(() => dateGroups.map(g => g.items.length), [dateGroups])

  // Active item index in flat list
  const activeIndex = useMemo(() => {
    if (!activeItemId) return -1
    return flatItems.findIndex(it => it.id === activeItemId)
  }, [flatItems, activeItemId])

  // Active item object
  const activeItem = useMemo(() => {
    if (activeIndex < 0) return null
    return flatItems[activeIndex] ?? null
  }, [flatItems, activeIndex])

  // Durable transfer status for the active file entry (gates Copy action)
  const activeEntryStatus = useAppSelector(state =>
    activeItemId ? selectEntryTransferStatus(state, activeItemId) : undefined
  )
  const isActiveFileCopyBlocked =
    activeItem?.type === 'file' &&
    activeEntryStatus != null &&
    activeEntryStatus.status !== 'completed'

  // 统一的 active 设置入口: 同时更新 anchoredToTopRef, 避免多处 setActiveItemId
  // 之间漏改导致锚状态偏离实际选中。
  const selectItem = useCallback(
    (id: string | null) => {
      setActiveItemId(id)
      anchoredToTopRef.current = id !== null && flatItems[0]?.id === id
    },
    [flatItems]
  )

  // 列表变化时维护 active:
  //   1. 空列表 → 清空, 并把锚重置为 true(下次有数据时自动落到新顶)。
  //   2. active 已不在列表 → 选第一项。
  //   3. 用户的锚还在顶, 但新内容把第一项顶下去了 → 跟到新顶。
  useEffect(() => {
    if (flatItems.length === 0) {
      if (activeItemId !== null) setActiveItemId(null)
      anchoredToTopRef.current = true
      return
    }
    const firstId = flatItems[0].id

    if (activeItemId === null || !flatItems.some(it => it.id === activeItemId)) {
      setActiveItemId(firstId)
      anchoredToTopRef.current = true
      return
    }

    if (anchoredToTopRef.current && activeItemId !== firstId) {
      setActiveItemId(firstId)
    }
  }, [flatItems, activeItemId])

  // Scroll active item into view. With virtualization the off-screen row has
  // no DOM node to scrollIntoView, so we drive the scroller imperatively by
  // the item's flat index. `scrollIntoView` (vs `scrollToIndex`) keeps the
  // original `block: 'nearest'` semantics: an already-visible row isn't
  // scrolled, and an off-screen one only scrolls just enough — no forced
  // centering (issue #1129 follow-up).
  useEffect(() => {
    if (activeIndex < 0) return
    virtuosoRef.current?.scrollIntoView({ index: activeIndex, behavior: 'smooth' })
    // Only re-scroll on an actual selection change; activeIndex is read fresh
    // from the same render, so unrelated list shifts (e.g. a prepend) don't
    // yank the viewport.
  }, [activeItemId])

  // Keyboard: Arrow Down
  useShortcut({
    key: 'down',
    scope: 'clipboard',
    handler: () => {
      if (flatItems.length === 0) return
      const nextIndex = activeIndex < 0 ? 0 : Math.min(activeIndex + 1, flatItems.length - 1)
      selectItem(flatItems[nextIndex].id)
    },
  })

  // Keyboard: Arrow Up
  useShortcut({
    key: 'up',
    scope: 'clipboard',
    handler: () => {
      if (flatItems.length === 0) return
      const prevIndex = activeIndex <= 0 ? 0 : activeIndex - 1
      selectItem(flatItems[prevIndex].id)
    },
  })

  // Copy
  const handleCopyItem = useCallback(
    async (itemId: string) => {
      try {
        captureUserIntent('copy_clipboard', { count: 1 })

        // For file entries, use the dedicated file copy command
        const item = flatItems.find(it => it.id === itemId)
        if (item?.type === 'file') {
          try {
            await copyFileToClipboard(itemId)
            setCopySuccess(true)
            setTimeout(() => setCopySuccess(false), 1500)
            return true
          } catch (err) {
            // 410 Gone (PAYLOAD_UNAVAILABLE) 表示后端识别出"内容不可用"——对文件类
            // entry,几乎专属于"用户已把本地源文件删除或移动"。把后端原始字面值
            // `payload_unavailable` 透传给用户毫无信息量,这里改成面向用户的明确
            // 文案;其他错误保留原始 message 用于排障。
            const { DaemonApiError, DaemonErrorCode } = await import('@/api/daemon/errors')
            const isPayloadUnavailable =
              err instanceof DaemonApiError && err.code === DaemonErrorCode.PAYLOAD_UNAVAILABLE
            const description = isPayloadUnavailable
              ? t('clipboard.errors.fileSourceMissing')
              : err instanceof Error
                ? err.message
                : String(err)
            dispatch(markEntryStale(itemId))
            toast.error(t('clipboard.errors.copyFailed'), {
              description,
            })
            return false
          }
        }

        const result = await dispatch(copyToClipboard(itemId)).unwrap()
        if (result.success) {
          setCopySuccess(true)
          setTimeout(() => setCopySuccess(false), 1500)
        }
        return result.success
      } catch (err) {
        log.error({ err }, 'Copy failed')
        toast.error(t('clipboard.errors.copyFailed'), {
          description:
            typeof err === 'string'
              ? err
              : err instanceof Error
                ? err.message
                : t('clipboard.errors.unknown'),
        })
        return false
      }
    },
    [dispatch, t, flatItems]
  )

  // Open file location in system file manager
  const handleOpenFileLocation = useCallback(
    async (itemId: string) => {
      const path = firstRevealableFilePath(flatItems.find(it => it.id === itemId)?.content ?? null)
      if (!path) {
        toast.error(t('clipboard.errors.openLocationFailed'), {
          description: t('clipboard.errors.fileLocationUnavailable'),
        })
        return
      }
      try {
        await openFileLocation(path)
      } catch (err) {
        log.error({ err }, 'Open file location failed')
        toast.error(t('clipboard.errors.openLocationFailed'), {
          description: err instanceof Error ? err.message : t('clipboard.errors.unknown'),
        })
      }
    },
    [flatItems, t]
  )

  // Keyboard: C to copy (blocked for non-completed file entries)
  useShortcut({
    key: 'c',
    scope: 'clipboard',
    enabled: activeItemId !== null && !isActiveFileCopyBlocked,
    handler: () => {
      if (activeItemId && !isActiveFileCopyBlocked) void handleCopyItem(activeItemId)
    },
    preventDefault: false,
  })

  // Keyboard: D to delete
  useShortcut({
    key: 'd',
    scope: 'clipboard',
    enabled: activeItemId !== null,
    handler: () => {
      if (activeItemId) {
        captureUserIntent('delete_entry', { count: 1 })
        setDeleteDialogOpen(true)
      }
    },
    preventDefault: false,
  })

  const handleConfirmDelete = async () => {
    if (!activeItemId) return
    try {
      await dispatch(removeClipboardItem(activeItemId)).unwrap()
      // Select next or previous item
      if (flatItems.length > 1) {
        const nextIndex = activeIndex < flatItems.length - 1 ? activeIndex + 1 : activeIndex - 1
        selectItem(flatItems[nextIndex]?.id ?? null)
      } else {
        selectItem(null)
      }
    } catch (e) {
      log.error({ err: e }, 'Delete failed')
    }
  }

  // Virtuoso fires this as the user approaches the end of the rendered range.
  const handleEndReached = useCallback(() => {
    if (!onLoadMore || !hasMore || loading || notReady) return
    onLoadMore()
  }, [hasMore, loading, notReady, onLoadMore])

  // Stable per-row handlers so memoized ClipboardListRow children don't
  // re-render on every parent render (selection / clock tick).
  const handleRowCopy = useCallback((id: string) => void handleCopyItem(id), [handleCopyItem])
  const handleRowOpenLocation = useCallback(
    (id: string) => void handleOpenFileLocation(id),
    [handleOpenFileLocation]
  )
  const handleRowDelete = useCallback(
    (id: string) => {
      selectItem(id)
      captureUserIntent('delete_entry', { count: 1 })
      setDeleteDialogOpen(true)
    },
    [selectItem]
  )

  return (
    <div className="h-full flex flex-col">
      {/* Search result count */}
      {isSearchActive && searchTotal !== null && !searchLoading && (
        <div className="shrink-0 px-4 py-1.5 text-xs text-muted-foreground border-b border-border/30">
          {searchTotal} {t('clipboard.search.resultsCount')}
        </div>
      )}

      {/* Search loading indicator */}
      {searchLoading && clipboardItems.length === 0 && (
        <div className="flex-1 flex items-center justify-center">
          <Loader2 className="size-5 animate-spin text-muted-foreground mr-2" />
          <span className="text-sm text-muted-foreground">{t('clipboard.search.searching')}</span>
        </div>
      )}

      {!searchLoading && clipboardItems.length > 0 ? (
        <ResizablePanelGroup
          id="clipboard-panels"
          orientation="horizontal"
          defaultLayout={defaultLayout}
          onLayoutChanged={onLayoutChanged}
          className={cn('flex-1 min-h-0', isWindows && 'overflow-hidden')}
        >
          {/* Left panel: virtualized item list. Only the rows in (and just
              around) the viewport are mounted, so a long history no longer
              janks weak machines on scroll/reconcile (issue #1129). */}
          <ResizablePanel id="clipboard-list" defaultSize="40%" minSize="25%" maxSize="60%">
            <GroupedVirtuoso
              ref={virtuosoRef}
              style={{ height: '100%' }}
              className={cn('no-scrollbar', isWindows ? 'bg-transparent' : 'bg-muted/20')}
              groupCounts={groupCounts}
              endReached={handleEndReached}
              increaseViewportBy={300}
              components={{
                Header: () => <div className="h-3" />,
                Footer: () => <div className="h-3" />,
              }}
              groupContent={index => (
                // Opaque `bg-card` base so the sticky header occludes rows
                // scrolling under it, plus the same `bg-muted/20` tint the list
                // scroller carries (non-Windows) — together they match the list
                // background exactly instead of reading as a different color.
                <div className="bg-card">
                  <div
                    className={cn(
                      'px-6 py-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground',
                      !isWindows && 'bg-muted/20'
                    )}
                  >
                    {dateGroups[index]?.label}
                  </div>
                </div>
              )}
              itemContent={index => {
                const item = flatItems[index]
                if (!item) return null
                return (
                  <div className="px-3 pb-0.5">
                    <ClipboardListRow
                      item={item}
                      isActive={item.id === activeItemId}
                      isStale={staleEntryIds.includes(item.id)}
                      onSelect={selectItem}
                      onCopy={handleRowCopy}
                      onDelete={handleRowDelete}
                      onOpenFileLocation={handleRowOpenLocation}
                    />
                  </div>
                )
              }}
            />
          </ResizablePanel>

          <ResizableHandle />

          {/* Right panel: preview + action bar */}
          <ResizablePanel id="clipboard-preview" defaultSize="60%" minSize="30%">
            <div className="h-full flex flex-col min-w-0">
              <ClipboardPreview
                item={activeItem}
                actions={
                  <ClipboardActionBar
                    hasActiveItem={activeItemId !== null}
                    copySuccess={copySuccess}
                    transferStatus={{
                      isCopyBlocked: isActiveFileCopyBlocked,
                      copyBlockedReason:
                        isActiveFileCopyBlocked && activeEntryStatus
                          ? activeEntryStatus.status === 'pending'
                            ? t('clipboard.transfer.copyDisabled.pending')
                            : activeEntryStatus.status === 'transferring'
                              ? t('clipboard.transfer.copyDisabled.transferring')
                              : t('clipboard.transfer.copyDisabled.failed')
                          : undefined,
                    }}
                    onCopy={() => {
                      if (activeItemId && !isActiveFileCopyBlocked)
                        void handleCopyItem(activeItemId)
                    }}
                    onDelete={() => {
                      if (activeItemId) {
                        captureUserIntent('delete_entry', { count: 1 })
                        setDeleteDialogOpen(true)
                      }
                    }}
                  />
                }
              />
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      ) : !searchLoading ? (
        <div className="mx-auto flex h-full w-full max-w-xl flex-col items-center justify-center text-center">
          <div className="mb-5 rounded-full bg-muted/30 p-5 ring-1 ring-border/50">
            {searchQuery ? (
              <Search className="size-10 text-muted-foreground/50" />
            ) : (
              <Inbox className="size-10 text-muted-foreground/50" />
            )}
          </div>
          <h3 className="mb-2 text-xl font-semibold text-foreground">
            {searchQuery
              ? t('clipboard.search.noResults', { query: searchQuery })
              : t('clipboard.search.empty')}
          </h3>
          <p className="max-w-sm text-muted-foreground">
            {searchQuery ? t('clipboard.search.noResultsSub') : t('clipboard.search.emptySub')}
          </p>
        </div>
      ) : null}

      <DeleteConfirmDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        onConfirm={handleConfirmDelete}
        count={1}
      />
    </div>
  )
}

export default ClipboardContent
