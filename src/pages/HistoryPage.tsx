import { LayoutGroup, m } from 'framer-motion'
import { Code, ExternalLink, File, FileText, Image as ImageIcon, Search } from 'lucide-react'
import React, { useMemo, useState, useCallback, useRef, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import Masonry from 'react-masonry-css'
import { Filter } from '@/api/clipboardItems'
import DeleteConfirmDialog from '@/components/clipboard/DeleteConfirmDialog'
import HistoryCard from '@/components/history/HistoryCard'
import { toast } from '@/components/ui/toast'
import { useClipboardEvents } from '@/hooks/useClipboardEvents'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import type {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { copyToClipboard, removeClipboardItem } from '@/store/slices/clipboardSlice'

// ── Constants ───────────────────────────────────────────────────

const FILTER_TABS: { key: Filter; labelKey: string; icon?: React.ElementType }[] = [
  { key: Filter.All, labelKey: 'history.filter.all' },
  { key: Filter.Text, labelKey: 'history.filter.text', icon: FileText },
  { key: Filter.Code, labelKey: 'history.filter.code', icon: Code },
  { key: Filter.Link, labelKey: 'history.filter.link', icon: ExternalLink },
  { key: Filter.Image, labelKey: 'history.filter.image', icon: ImageIcon },
  { key: Filter.File, labelKey: 'history.filter.file', icon: File },
]

// ── Constants ──────────────────────────────────────────────────

const MASONRY_BREAKPOINTS = { default: 3, 900: 2, 500: 1 }

// ── Page ────────────────────────────────────────────────────────

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const [activeFilter, setActiveFilter] = useState<Filter>(Filter.All)
  const [searchQuery, setSearchQuery] = useState('')
  const [hoveredId, setHoveredId] = useState<string | null>(null)
  const [copySuccessId, setCopySuccessId] = useState<string | null>(null)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [promotedId, setPromotedId] = useState<string | null>(null)
  const deleteTargetRef = useRef<string | null>(null)
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useShortcutScope('clipboard')
  const { hasMore, handleLoadMore } = useClipboardEvents(activeFilter)

  const items = useAppSelector(state => state.clipboard.items)

  const formatRelativeTime = useCallback(
    (timestamp: number): string => {
      const diff = Date.now() - timestamp
      const minutes = Math.floor(diff / 60000)
      if (minutes < 1) return t('clipboard.time.justNow')
      if (minutes < 60) return t('clipboard.time.minutesAgo', { minutes })
      const hours = Math.floor(minutes / 60)
      if (hours < 24) return t('clipboard.time.hoursAgo', { hours })
      return t('clipboard.time.daysAgo', { days: Math.floor(hours / 24) })
    },
    [t]
  )

  const displayItems = useMemo<DisplayClipboardItem[]>(
    () =>
      items.map(entry => ({
        id: entry.id,
        type: entry.type,
        content: entry.content,
        time: formatRelativeTime(entry.activeTime),
        activeTime: entry.activeTime,
        isFavorited: entry.isFavorited,
        isUnavailable: entry.isUnavailable,
      })),
    [items, formatRelativeTime]
  )

  const filteredItems = useMemo(() => {
    if (!searchQuery.trim()) return displayItems
    const q = searchQuery.toLowerCase()
    return displayItems.filter(item => {
      if (!item.content) return false
      switch (item.type) {
        case 'text':
          return (item.content as ClipboardTextItem).display_text.toLowerCase().includes(q)
        case 'code':
          return (item.content as ClipboardCodeItem).code.toLowerCase().includes(q)
        case 'link': {
          const l = item.content as ClipboardLinkItem
          return (
            l.urls.some(u => u.toLowerCase().includes(q)) ||
            l.domains.some(d => d.toLowerCase().includes(q))
          )
        }
        case 'file':
          return (item.content as ClipboardFileItem).file_names.some(n =>
            n.toLowerCase().includes(q)
          )
        default:
          return false
      }
    })
  }, [displayItems, searchQuery])

  const orderedItems = useMemo(() => {
    if (!promotedId) return filteredItems
    const idx = filteredItems.findIndex(it => it.id === promotedId)
    if (idx <= 0) return filteredItems
    return [filteredItems[idx], ...filteredItems.slice(0, idx), ...filteredItems.slice(idx + 1)]
  }, [filteredItems, promotedId])

  // ── Infinite scroll ───────────────────────────────────────────
  const scrollRef = useRef<HTMLDivElement>(null)
  const hasMoreRef = useRef(hasMore)
  const handleLoadMoreRef = useRef(handleLoadMore)

  useEffect(() => {
    hasMoreRef.current = hasMore
  }, [hasMore])

  useEffect(() => {
    handleLoadMoreRef.current = handleLoadMore
  }, [handleLoadMore])

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    const onScroll = () => {
      if (!hasMoreRef.current) return
      const { scrollTop, scrollHeight, clientHeight } = el
      if (scrollHeight - scrollTop - clientHeight < 400) {
        handleLoadMoreRef.current()
      }
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => el.removeEventListener('scroll', onScroll)
  }, [])

  // ── Copy handler ──────────────────────────────────────────────
  const handleCopy = useCallback(
    (id: string) => {
      dispatch(copyToClipboard(id))
        .unwrap()
        .then(() => {
          if (copyTimerRef.current) clearTimeout(copyTimerRef.current)
          setCopySuccessId(id)
          setPromotedId(id)
          copyTimerRef.current = setTimeout(() => setCopySuccessId(null), 1200)
        })
        .catch(() => toast.error(t('clipboard.errors.copyFailed')))
    },
    [dispatch, t]
  )

  // ── Delete handler ────────────────────────────────────────────
  const handleConfirmDelete = useCallback(async () => {
    const targetId = deleteTargetRef.current
    if (!targetId) return
    setDeletingId(targetId)
    setTimeout(async () => {
      try {
        await dispatch(removeClipboardItem(targetId)).unwrap()
      } catch {
        toast.error(t('clipboard.errors.deleteFailed', 'Delete failed'))
      }
      setDeletingId(null)
      deleteTargetRef.current = null
    }, 400)
  }, [dispatch, t])

  const handleDelete = useCallback((id: string) => {
    deleteTargetRef.current = id
    setDeleteDialogOpen(true)
  }, [])

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
      if (hoveredId) handleDelete(hoveredId)
    },
    preventDefault: false,
  })

  const totalCount = displayItems.length

  return (
    <div className="flex flex-col h-full">
      {/* ── Toolbar: search + filters ──────────────────────────── */}
      <div className="shrink-0 flex items-center gap-2 px-4 py-2 border-b border-border/20">
        {/* Search */}
        <div className="flex items-center gap-1.5 bg-muted/40 rounded-lg px-2.5 h-7 w-48 focus-within:bg-muted/60 transition-colors">
          <Search className="size-3.5 text-muted-foreground/50 shrink-0" />
          <input
            type="text"
            value={searchQuery}
            onChange={e => setSearchQuery(e.target.value)}
            placeholder={t('history.searchPlaceholder')}
            className="flex-1 bg-transparent text-[12px] text-foreground placeholder:text-muted-foreground/50 outline-none min-w-0"
          />
        </div>

        <div className="w-px h-4 bg-border/30" />

        {/* Filters */}
        {FILTER_TABS.map(tab => {
          const isActive = activeFilter === tab.key
          return (
            <button
              key={tab.key}
              type="button"
              onClick={() => setActiveFilter(tab.key)}
              className={cn(
                'flex items-center gap-1.5 h-7 px-2.5 rounded-lg text-[12px] font-medium whitespace-nowrap transition-all duration-150',
                isActive
                  ? 'bg-foreground/8 text-foreground'
                  : 'text-muted-foreground/60 hover:text-muted-foreground hover:bg-muted/40'
              )}
            >
              {tab.icon && <tab.icon className="size-3" />}
              {t(tab.labelKey)}
              {tab.key === Filter.All && totalCount > 0 && (
                <span className="text-[10px] tabular-nums text-muted-foreground/40 ml-0.5">
                  {totalCount}
                </span>
              )}
            </button>
          )
        })}
      </div>

      {/* ── Grid ───────────────────────────────────────────────── */}
      <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto">
        {orderedItems.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 pb-10">
            <div className="size-12 rounded-2xl bg-muted/30 flex items-center justify-center">
              <Search className="size-5 text-muted-foreground/30" />
            </div>
            <div className="text-center space-y-1">
              <p className="text-[13px] font-medium">{t('clipboard.content.noClipboardItems')}</p>
              <p className="text-[12px] text-muted-foreground/50">
                {t('clipboard.content.emptyDescription')}
              </p>
            </div>
          </div>
        ) : (
          <LayoutGroup>
            <Masonry
              breakpointCols={MASONRY_BREAKPOINTS}
              className="flex px-2 pt-1 pb-4 [&>div+div]:border-l-[3px] [&>div+div]:border-double [&>div+div]:border-border/20"
              columnClassName="flex-1 min-w-0 flex flex-col"
            >
              {orderedItems.map(item => (
                <m.div
                  key={item.id}
                  layoutId={item.id}
                  layout
                  transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                >
                  <HistoryCard
                    item={item}
                    isHovered={hoveredId === item.id}
                    copySuccess={copySuccessId === item.id}
                    isDeleting={deletingId === item.id}
                    onCopy={handleCopy}
                    onHoverChange={setHoveredId}
                  />
                </m.div>
              ))}
            </Masonry>
          </LayoutGroup>
        )}
      </div>

      <DeleteConfirmDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        onConfirm={handleConfirmDelete}
        count={1}
      />
    </div>
  )
}

export default HistoryPage
