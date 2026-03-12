import { Inbox } from 'lucide-react'
import React, { useMemo, useState, useEffect, useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useDefaultLayout } from 'react-resizable-panels'
import ClipboardActionBar from './ClipboardActionBar'
import ClipboardItemRow from './ClipboardItemRow'
import ClipboardPreview from './ClipboardPreview'
import DeleteConfirmDialog from './DeleteConfirmDialog'
import {
  getDisplayType,
  ClipboardItemResponse,
  Filter,
  ClipboardTextItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardCodeItem,
  ClipboardFileItem,
} from '@/api/clipboardItems'
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable'
import { toast } from '@/components/ui/toast'
import { useShortcut } from '@/hooks/useShortcut'
import { captureUserIntent } from '@/observability/breadcrumbs'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { removeClipboardItem, copyToClipboard } from '@/store/slices/clipboardSlice'

export interface DisplayClipboardItem {
  id: string
  type: 'text' | 'image' | 'link' | 'code' | 'file' | 'unknown'
  time: string
  activeTime: number
  isDownloaded?: boolean
  isFavorited?: boolean
  content:
    | ClipboardTextItem
    | ClipboardImageItem
    | ClipboardLinkItem
    | ClipboardCodeItem
    | ClipboardFileItem
    | null
  device?: string
}

interface DateGroup {
  label: string
  items: DisplayClipboardItem[]
}

interface ClipboardContentProps {
  filter: Filter
  searchQuery?: string
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

const ClipboardContent: React.FC<ClipboardContentProps> = ({
  filter,
  searchQuery = '',
  hasMore = true,
  onLoadMore,
}) => {
  const { t } = useTranslation()

  const dispatch = useAppDispatch()

  // Persist panel layout to localStorage
  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: 'clipboard-panels',
    panelIds: ['clipboard-list', 'clipboard-preview'],
    storage: localStorage,
  })
  const { items: reduxItems, loading, notReady } = useAppSelector(state => state.clipboard)

  const [activeItemId, setActiveItemId] = useState<string | null>(null)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [copySuccess, setCopySuccess] = useState(false)

  const activeItemRef = useRef<HTMLDivElement>(null)
  const prevFirstItemIdRef = useRef<string | null>(null)

  // Convert clipboard item to display item
  const convertToDisplayItem = useCallback(
    (item: ClipboardItemResponse): DisplayClipboardItem => {
      const type = getDisplayType(item.item)

      const activeTime = new Date(item.active_time)
      const now = new Date()
      const diffMs = now.getTime() - activeTime.getTime()
      const diffMins = Math.round(diffMs / 60000)

      let timeString: string
      if (diffMins < 1) {
        timeString = t('clipboard.time.justNow')
      } else if (diffMins < 60) {
        timeString = t('clipboard.time.minutesAgo', { minutes: diffMins })
      } else if (diffMins < 1440) {
        timeString = t('clipboard.time.hoursAgo', { hours: Math.floor(diffMins / 60) })
      } else {
        timeString = t('clipboard.time.daysAgo', { days: Math.floor(diffMins / 1440) })
      }

      const contentByType = {
        text: item.item.text,
        image: item.item.image,
        link: item.item.link,
        code: item.item.code,
        file: item.item.file,
        unknown: null,
      } as const

      return {
        id: item.id,
        type,
        time: timeString,
        activeTime: item.active_time,
        isDownloaded: item.is_downloaded,
        isFavorited: item.is_favorited,
        content: contentByType[type] ?? null,
      }
    },
    [t]
  )

  // Build display items from Redux state
  const clipboardItems = useMemo(() => {
    if (!reduxItems || reduxItems.length === 0) return []

    let items: DisplayClipboardItem[] = reduxItems.map(convertToDisplayItem)

    if (filter === Filter.Favorited) {
      items = items.filter(it => it.isFavorited)
    }

    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase().trim()
      items = items.filter(it => {
        if (it.type === 'text' && it.content) {
          return (it.content as ClipboardTextItem).display_text?.toLowerCase().includes(query)
        }
        if (it.type === 'code' && it.content) {
          return (it.content as ClipboardCodeItem).code?.toLowerCase().includes(query)
        }
        if (it.type === 'link' && it.content) {
          return (it.content as ClipboardLinkItem).url?.toLowerCase().includes(query)
        }
        if (it.type === 'file' && it.content) {
          return (it.content as ClipboardFileItem).file_names?.some(name =>
            name.toLowerCase().includes(query)
          )
        }
        return false
      })
    }

    return items
  }, [reduxItems, filter, searchQuery, convertToDisplayItem])

  // Flat list for keyboard navigation
  const flatItems = useMemo(() => clipboardItems, [clipboardItems])

  // Date groups for rendering
  const dateGroups = useMemo(() => groupItemsByDate(clipboardItems, t), [clipboardItems, t])

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

  // Auto-select first item when list loads or changes
  useEffect(() => {
    const currentFirstId = flatItems.length > 0 ? flatItems[0].id : null

    if (flatItems.length > 0) {
      // Auto-follow: active was on the old first item, and a new item took over position 0
      if (
        prevFirstItemIdRef.current !== null &&
        activeItemId === prevFirstItemIdRef.current &&
        currentFirstId !== prevFirstItemIdRef.current
      ) {
        setActiveItemId(currentFirstId)
        prevFirstItemIdRef.current = currentFirstId
        return
      }
      // Auto-select: if no active item or active item no longer in list
      if (activeItemId === null || !flatItems.some(it => it.id === activeItemId)) {
        setActiveItemId(flatItems[0].id)
      }
    }
    if (flatItems.length === 0) {
      setActiveItemId(null)
    }

    prevFirstItemIdRef.current = currentFirstId
  }, [flatItems, activeItemId])

  // Scroll active item into view
  useEffect(() => {
    activeItemRef.current?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }, [activeItemId])

  // Keyboard: Arrow Down
  useShortcut({
    key: 'down',
    scope: 'clipboard',
    handler: () => {
      if (flatItems.length === 0) return
      const nextIndex = activeIndex < 0 ? 0 : Math.min(activeIndex + 1, flatItems.length - 1)
      setActiveItemId(flatItems[nextIndex].id)
    },
  })

  // Keyboard: Arrow Up
  useShortcut({
    key: 'up',
    scope: 'clipboard',
    handler: () => {
      if (flatItems.length === 0) return
      const prevIndex = activeIndex <= 0 ? 0 : activeIndex - 1
      setActiveItemId(flatItems[prevIndex].id)
    },
  })

  // Copy
  const handleCopyItem = useCallback(
    async (itemId: string) => {
      try {
        captureUserIntent('copy_clipboard', { count: 1 })
        const result = await dispatch(copyToClipboard(itemId)).unwrap()
        if (result.success) {
          setCopySuccess(true)
          setTimeout(() => setCopySuccess(false), 1500)
        }
        return result.success
      } catch (err) {
        console.error('Copy failed:', err)
        toast.error(t('clipboard.errors.copyFailed'), {
          description: err instanceof Error ? err.message : t('clipboard.errors.unknown'),
        })
        return false
      }
    },
    [dispatch, t]
  )

  // Keyboard: C to copy
  useShortcut({
    key: 'c',
    scope: 'clipboard',
    enabled: activeItemId !== null,
    handler: () => {
      if (activeItemId) void handleCopyItem(activeItemId)
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
        setActiveItemId(flatItems[nextIndex]?.id ?? null)
      } else {
        setActiveItemId(null)
      }
    } catch (e) {
      console.error('Delete failed:', e)
    }
  }

  const handleScroll = useCallback(
    (event: React.UIEvent<HTMLDivElement>) => {
      if (!onLoadMore || !hasMore || loading || notReady) return
      const target = event.currentTarget
      const remaining = target.scrollHeight - target.scrollTop - target.clientHeight
      if (remaining <= 200) {
        onLoadMore()
      }
    },
    [hasMore, loading, notReady, onLoadMore]
  )

  return (
    <div className="h-full flex flex-col">
      {clipboardItems.length > 0 ? (
        <ResizablePanelGroup
          id="clipboard-panels"
          orientation="horizontal"
          defaultLayout={defaultLayout}
          onLayoutChanged={onLayoutChanged}
          className="flex-1 min-h-0"
        >
          {/* Left panel: item list */}
          <ResizablePanel id="clipboard-list" defaultSize="40%" minSize="25%" maxSize="60%">
            <div
              className="h-full bg-muted/20 overflow-y-auto overflow-x-hidden no-scrollbar"
              onScroll={handleScroll}
            >
              <div className="p-3 flex flex-col gap-0.5">
                {dateGroups.map(group => (
                  <div key={group.label}>
                    <div className="px-3 py-2 text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                      {group.label}
                    </div>
                    {group.items.map(item => (
                      <ClipboardItemRow
                        key={item.id}
                        item={item}
                        isActive={item.id === activeItemId}
                        onClick={() => setActiveItemId(item.id)}
                        itemRef={item.id === activeItemId ? activeItemRef : undefined}
                      />
                    ))}
                  </div>
                ))}
              </div>
            </div>
          </ResizablePanel>

          <ResizableHandle />

          {/* Right panel: preview + action bar */}
          <ResizablePanel id="clipboard-preview" defaultSize="60%" minSize="30%">
            <div className="h-full flex flex-col min-w-0">
              <ClipboardPreview item={activeItem} />
              <ClipboardActionBar
                hasActiveItem={activeItemId !== null}
                copySuccess={copySuccess}
                onCopy={() => {
                  if (activeItemId) void handleCopyItem(activeItemId)
                }}
                onDelete={() => {
                  if (activeItemId) {
                    captureUserIntent('delete_entry', { count: 1 })
                    setDeleteDialogOpen(true)
                  }
                }}
              />
            </div>
          </ResizablePanel>
        </ResizablePanelGroup>
      ) : (
        <div className="mx-auto flex h-full w-full max-w-xl flex-col items-center justify-center text-center">
          <div className="mb-5 rounded-full bg-muted/30 p-5 ring-1 ring-border/50">
            <Inbox className="h-10 w-10 text-muted-foreground/50" />
          </div>
          <h3 className="mb-2 text-xl font-semibold text-foreground">
            {t('clipboard.content.noClipboardItems')}
          </h3>
          <p className="max-w-sm text-muted-foreground">
            {t('clipboard.content.emptyDescription')}
          </p>
        </div>
      )}

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
