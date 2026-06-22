import { Code, Copy, ExternalLink, File, FileText, Image as ImageIcon, Search } from 'lucide-react'
import React, { useMemo, useState, useCallback, useRef, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Filter } from '@/api/clipboardItems'
import { toast } from '@/components/ui/toast'
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

const TYPE_COLOR: Record<string, string> = {
  text: 'rgb(140,150,160)',
  code: 'rgb(140,120,210)',
  link: 'rgb(70,145,210)',
  image: 'rgb(80,160,110)',
  file: 'rgb(175,140,100)',
  unknown: 'rgb(140,150,160)',
}

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

// ── Content renderers ───────────────────────────────────────────

const TextContent: React.FC<{ item: ClipboardTextItem }> = ({ item }) => {
  const isMasked = /^[•·*]{6,}$/.test(item.display_text.trim())
  return (
    <div className="text-[13px] leading-[1.55] text-foreground/85 line-clamp-4">
      {isMasked ? (
        <span className="tracking-[0.12em] text-muted-foreground/70 select-none">
          {item.display_text}
        </span>
      ) : (
        item.display_text
      )}
    </div>
  )
}

const CodeContent: React.FC<{ item: ClipboardCodeItem }> = ({ item }) => (
  <pre className="rounded-lg bg-[#1a1726] px-3 py-2.5 text-[10.5px] leading-[1.6] text-[#c8c0e0] line-clamp-5 font-mono -mx-0.5">
    <code>{item.code}</code>
  </pre>
)

const LinkContent: React.FC<{ item: ClipboardLinkItem }> = ({ item }) => {
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
    <div className="space-y-0.5">
      <div className="text-[13px] font-medium text-foreground/85 leading-snug line-clamp-2">
        {title}
      </div>
      <div className="flex items-center gap-1 text-[11px] text-muted-foreground/70">
        <ExternalLink className="size-[10px] shrink-0" />
        <span className="truncate">{domain}</span>
      </div>
    </div>
  )
}

const ImageContent: React.FC<{ item: ClipboardImageItem }> = ({ item }) => (
  <div className="relative rounded-lg overflow-hidden bg-muted/20 -mx-0.5">
    {item.thumbnail ? (
      <img src={item.thumbnail} alt="" className="w-full object-cover max-h-[140px]" />
    ) : (
      <div className="h-[90px] flex items-center justify-center">
        <ImageIcon className="size-7 text-muted-foreground/25" />
      </div>
    )}
    {item.width > 0 && item.height > 0 && (
      <span className="absolute bottom-1.5 left-2 text-[10px] font-medium text-white/80 drop-shadow-[0_1px_3px_rgba(0,0,0,0.6)]">
        {item.width}×{item.height}
      </span>
    )}
  </div>
)

const FileContent: React.FC<{ item: ClipboardFileItem }> = ({ item }) => {
  const name = item.file_names[0] ?? 'Unknown file'
  const size = item.file_sizes[0] ?? 0
  return (
    <div className="flex items-center gap-2">
      <File className="size-4 text-muted-foreground/50 shrink-0" />
      <div className="min-w-0 flex-1">
        <div className="text-[12.5px] font-medium text-foreground/85 truncate">{name}</div>
        <div className="text-[10.5px] text-muted-foreground/60">
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
  const color = TYPE_COLOR[item.type] ?? TYPE_COLOR.unknown
  const TypeIcon = TYPE_ICONS[item.type] ?? FileText

  const content = (() => {
    if (!item.content) return null
    switch (item.type) {
      case 'text':
        return <TextContent item={item.content as ClipboardTextItem} />
      case 'code':
        return <CodeContent item={item.content as ClipboardCodeItem} />
      case 'link':
        return <LinkContent item={item.content as ClipboardLinkItem} />
      case 'image':
        return <ImageContent item={item.content as ClipboardImageItem} />
      case 'file':
        return <FileContent item={item.content as ClipboardFileItem} />
      default:
        return item.textPreview ? (
          <div className="text-[13px] text-muted-foreground/70 line-clamp-3">
            {item.textPreview}
          </div>
        ) : null
    }
  })()

  return (
    <div className="group relative px-3.5 pt-3 pb-3 border-b-[3px] border-double border-border/30 hover:bg-muted/30 transition-colors duration-150">
      <div className="flex items-center gap-1.5 mb-1.5">
        <TypeIcon className="size-3" style={{ color }} />
        <span className="text-[10.5px] font-medium" style={{ color }}>
          {t(`history.type.${item.type}`, item.type)}
        </span>
        <span className="text-[10px] text-muted-foreground/40 ml-auto">{item.time}</span>
      </div>

      {content}

      <button
        type="button"
        onClick={() => onCopy(item.id)}
        className="absolute top-2.5 right-2.5 flex items-center justify-center size-6 rounded-md bg-card border border-border/50 text-muted-foreground shadow-sm opacity-0 group-hover:opacity-100 hover:text-foreground hover:border-border transition-all duration-150"
      >
        <Copy className="size-3" />
      </button>
    </div>
  )
}

// ── Page ─────────────────────────────────────────────────────────

const HistoryPage: React.FC = () => {
  const { t } = useTranslation()
  const dispatch = useAppDispatch()
  const [activeFilter, setActiveFilter] = useState<Filter>(Filter.All)
  const [searchQuery, setSearchQuery] = useState('')

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
        {filteredItems.length === 0 ? (
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
          <div className="flex px-2 pt-1 pb-4">
            {columns.map((col, ci) => (
              <div
                key={ci}
                className={cn(
                  'flex-1 min-w-0 flex flex-col',
                  ci > 0 && 'border-l-[3px] border-double border-border/20'
                )}
              >
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
