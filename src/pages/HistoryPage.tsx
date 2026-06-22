import {
  Code,
  Copy,
  ExternalLink,
  File,
  FileText,
  Image as ImageIcon,
  MoreHorizontal,
  Search,
} from 'lucide-react'
import React, { useMemo, useState, useCallback, useRef, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import { toast } from '@/components/ui/toast'
import { useSearch } from '@/contexts/search-context'
import { useClipboardEvents } from '@/hooks/useClipboardEvents'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import type {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'
import { useAppDispatch, useAppSelector } from '@/store/hooks'
import { copyToClipboard } from '@/store/slices/clipboardSlice'

// ── Design tokens ───────────────────────────────────────────────

const TYPE_THEME = {
  text: { color: 'rgb(124,139,151)', bg: 'rgba(124,139,151,0.12)' },
  code: { color: 'rgb(122,108,201)', bg: 'rgba(122,108,201,0.12)' },
  link: { color: 'rgb(62,143,208)', bg: 'rgba(62,143,208,0.12)' },
  image: { color: 'rgb(79,160,106)', bg: 'rgba(79,160,106,0.12)' },
  file: { color: 'rgb(168,130,90)', bg: 'rgba(168,130,90,0.12)' },
  unknown: { color: 'rgb(124,139,151)', bg: 'rgba(124,139,151,0.12)' },
} as const

const TYPE_ICONS: Record<string, React.ElementType> = {
  text: FileText,
  code: Code,
  link: ExternalLink,
  image: ImageIcon,
  file: File,
  unknown: FileText,
}

const FILTER_TABS: { key: Filter; labelKey: string; icon?: React.ElementType }[] = [
  { key: Filter.All, labelKey: 'history.filter.all' },
  { key: Filter.Text, labelKey: 'history.filter.text', icon: FileText },
  { key: Filter.Code, labelKey: 'history.filter.code', icon: Code },
  { key: Filter.Link, labelKey: 'history.filter.link', icon: ExternalLink },
  { key: Filter.Image, labelKey: 'history.filter.image', icon: ImageIcon },
  { key: Filter.File, labelKey: 'history.filter.file', icon: File },
]

// ── Helpers ──────────────────────────────────────────────────────

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function getFileExtLabel(name: string): string {
  return name.split('.').pop()?.toUpperCase() || 'FILE'
}

function distributeToColumns<T>(items: T[], colCount: number): T[][] {
  const cols: T[][] = Array.from({ length: colCount }, () => [])
  items.forEach((item, i) => cols[i % colCount].push(item))
  return cols
}

// ── Card content renderers ──────────────────────────────────────

const TextCardContent: React.FC<{ item: ClipboardTextItem }> = ({ item }) => {
  const isMasked = /^[•·*]{6,}$/.test(item.display_text.trim())
  return (
    <div className="mx-3 mb-1 rounded-[10px] bg-muted/50 border border-border/30 px-3 py-2.5">
      <div className="text-[13px] leading-[1.6] text-foreground/90 line-clamp-3">
        {isMasked ? (
          <span className="tracking-[0.15em] text-muted-foreground select-none">
            {item.display_text}
          </span>
        ) : (
          item.display_text
        )}
      </div>
    </div>
  )
}

const CodeCardContent: React.FC<{ item: ClipboardCodeItem }> = ({ item }) => (
  <div className="mx-3 mb-1 rounded-[10px] overflow-hidden">
    <pre className="bg-[#1e1b2e] px-3 py-2.5 text-[10.5px] leading-[1.65] text-[#d4ceeb] line-clamp-4 font-mono">
      <code>{item.code}</code>
    </pre>
  </div>
)

const LinkCardContent: React.FC<{ item: ClipboardLinkItem }> = ({ item }) => {
  const url = item.urls[0] ?? ''
  const domain = item.domains[0] ?? ''
  let title = url
  try {
    const u = new URL(url)
    title = u.pathname === '/' ? u.hostname : `${u.hostname}${u.pathname}`
  } catch {
    /* keep raw url */
  }
  return (
    <div className="mx-3 mb-1 rounded-[10px] bg-muted/50 border border-border/30 px-3 py-2.5 space-y-1">
      <div className="text-[12.5px] font-bold text-foreground/90 leading-snug line-clamp-2">
        {title}
      </div>
      <div
        className="flex items-center gap-1 text-[11px] font-semibold"
        style={{ color: TYPE_THEME.link.color }}
      >
        <ExternalLink className="size-[11px] shrink-0" />
        <span className="truncate">{domain}</span>
      </div>
    </div>
  )
}

const ImageCardContent: React.FC<{ item: ClipboardImageItem }> = ({ item }) => (
  <div className="mx-3 mb-1 rounded-[10px] overflow-hidden">
    <div className="relative bg-muted/30 h-[110px]">
      {item.thumbnail ? (
        <img src={item.thumbnail} alt="" className="w-full h-full object-cover" />
      ) : (
        <div className="w-full h-full flex items-center justify-center">
          <ImageIcon className="size-8 text-muted-foreground/30" />
        </div>
      )}
      {item.width > 0 && item.height > 0 && (
        <span className="absolute bottom-1.5 left-2 text-[10px] font-semibold text-white/90 drop-shadow-[0_1px_2px_rgba(0,0,0,0.5)]">
          {item.width}
          <span className="opacity-60 mx-px">×</span>
          {item.height}
        </span>
      )}
    </div>
  </div>
)

const FileCardContent: React.FC<{ item: ClipboardFileItem }> = ({ item }) => {
  const name = item.file_names[0] ?? 'Unknown file'
  const size = item.file_sizes[0] ?? 0
  return (
    <div className="mx-3 mb-1 rounded-[10px] bg-muted/50 border border-border/30 px-3 py-2.5 flex items-center gap-2.5">
      <div
        className="flex items-center justify-center size-8 rounded-lg shrink-0"
        style={{ backgroundColor: TYPE_THEME.file.bg, color: TYPE_THEME.file.color }}
      >
        <File className="size-4" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[12.5px] font-bold text-foreground/90 truncate">{name}</div>
        <div className="text-[11px] text-muted-foreground">
          {getFileExtLabel(name)} · {formatFileSize(size)}
        </div>
      </div>
    </div>
  )
}

// ── Card ─────────────────────────────────────────────────────────

const HistoryCard: React.FC<{
  item: DisplayClipboardItem
  onCopy: (id: string) => void
}> = ({ item, onCopy }) => {
  const { t } = useTranslation()
  const theme = TYPE_THEME[item.type] ?? TYPE_THEME.unknown
  const TypeIcon = TYPE_ICONS[item.type] ?? FileText
  const typeLabel = t(`history.type.${item.type}`, item.type)

  const content = (() => {
    if (!item.content) return null
    switch (item.type) {
      case 'text':
        return <TextCardContent item={item.content as ClipboardTextItem} />
      case 'code':
        return <CodeCardContent item={item.content as ClipboardCodeItem} />
      case 'link':
        return <LinkCardContent item={item.content as ClipboardLinkItem} />
      case 'image':
        return <ImageCardContent item={item.content as ClipboardImageItem} />
      case 'file':
        return <FileCardContent item={item.content as ClipboardFileItem} />
      default:
        return item.textPreview ? (
          <div className="mx-3 mb-1 rounded-[10px] bg-muted/50 border border-border/30 px-3 py-2.5 text-[13px] text-muted-foreground line-clamp-3">
            {item.textPreview}
          </div>
        ) : null
    }
  })()

  return (
    <div className="group rounded-xl border border-border/50 bg-card shadow-sm hover:shadow-md hover:border-border/80 transition-[shadow,border-color] duration-200 overflow-hidden">
      {/* ① Type header */}
      <div className="flex items-center gap-2 px-3 pt-3 pb-1.5">
        <span
          className="flex items-center justify-center size-6 rounded-md"
          style={{ backgroundColor: theme.bg, color: theme.color }}
        >
          <TypeIcon className="size-3.5" />
        </span>
        <span className="text-[11px] font-bold tracking-wide" style={{ color: theme.color }}>
          {typeLabel}
        </span>
        <div className="flex-1" />
        {item.isFavorited && (
          <span className="size-[7px] rounded-full bg-primary shadow-[0_0_0_2.5px] shadow-primary/15" />
        )}
      </div>

      {/* ② Content preview */}
      {content}

      {/* ③ Footer */}
      <div className="flex items-center px-3 pt-1.5 pb-2.5">
        <span className="text-[11px] text-muted-foreground">{item.time}</span>
        <div className="flex-1" />
        <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity duration-150">
          <button
            type="button"
            onClick={() => onCopy(item.id)}
            className="flex items-center justify-center size-[26px] rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 transition-colors"
          >
            <Copy className="size-[13px]" />
          </button>
          <button
            type="button"
            className="flex items-center justify-center size-[26px] rounded-lg bg-muted/80 border border-border/40 text-muted-foreground hover:text-foreground transition-colors"
          >
            <MoreHorizontal className="size-[13px]" />
          </button>
        </div>
      </div>
    </div>
  )
}

// ── Page ─────────────────────────────────────────────────────────

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const { searchValue: searchQuery } = useSearch()
  const [activeFilter, setActiveFilter] = useState<Filter>(Filter.All)

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

  const columns = useMemo(() => distributeToColumns(filteredItems, 3), [filteredItems])

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

  const handleCopy = useCallback(
    (id: string) => {
      dispatch(copyToClipboard(id))
        .unwrap()
        .then(() => toast.success(t('clipboard.item.actions.copy')))
        .catch(() => toast.error(t('clipboard.errors.copyFailed')))
    },
    [dispatch, t]
  )

  const totalCount = displayItems.length

  return (
    <div className="flex flex-col h-full">
      {/* ── Filter tabs ────────────────────────────────────────── */}
      <div className="shrink-0 flex items-center gap-1.5 px-5 py-2.5 border-b border-border/20">
        {FILTER_TABS.map(tab => {
          const isActive = activeFilter === tab.key
          return (
            <button
              key={tab.key}
              type="button"
              onClick={() => setActiveFilter(tab.key)}
              className={cn(
                'flex items-center gap-1.5 h-7 px-3 rounded-full text-[12px] font-semibold whitespace-nowrap transition-all duration-150',
                isActive
                  ? 'bg-primary text-primary-foreground shadow-[0_2px_8px_-1px] shadow-primary/30'
                  : 'text-muted-foreground hover:text-foreground hover:bg-muted/60'
              )}
            >
              {tab.icon && <tab.icon className="size-3" />}
              {t(tab.labelKey)}
              {tab.key === Filter.All && totalCount > 0 && (
                <span
                  className={cn(
                    'text-[10px] font-bold tabular-nums ml-0.5',
                    isActive ? 'text-primary-foreground/70' : 'text-muted-foreground/50'
                  )}
                >
                  {totalCount}
                </span>
              )}
            </button>
          )
        })}
      </div>

      {/* ── Card grid ──────────────────────────────────────────── */}
      <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto">
        {filteredItems.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 pb-10">
            <div className="size-12 rounded-2xl bg-muted/40 flex items-center justify-center">
              <Search className="size-5 text-muted-foreground/40" />
            </div>
            <div className="text-center space-y-1">
              <p className="text-[13px] font-medium">{t('clipboard.content.noClipboardItems')}</p>
              <p className="text-[12px] text-muted-foreground/60">
                {t('clipboard.content.emptyDescription')}
              </p>
            </div>
          </div>
        ) : (
          <div className="flex gap-3.5 px-5 pt-3.5 pb-5">
            {columns.map((col, ci) => (
              <div key={ci} className="flex-1 min-w-0 flex flex-col gap-3.5">
                {col.map(item => (
                  <HistoryCard key={item.id} item={item} onCopy={handleCopy} />
                ))}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

export default HistoryPage
