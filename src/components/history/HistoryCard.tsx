import {
  CheckCircle2,
  CircleDashed,
  Cloud,
  Code,
  Copy,
  ExternalLink,
  File,
  FileText,
  History,
  Image as ImageIcon,
  Laptop,
  LoaderCircle,
  AlertCircle,
} from 'lucide-react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { resolveResourceImageUrl } from '@/api/clipboardItems'
import { getClipboardEntryResource } from '@/api/daemon/clipboard'
import type {
  EntryDeliveryView,
  EntrySourceView,
  EntryDeliveryTargetView,
} from '@/api/tauri-command/clipboard_delivery'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import type {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'
import { formatFileSize } from '@/utils'

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

// ── Helpers ─────────────────────────────────────────────────────

function getFileExtLabel(name: string): string {
  return name.split('.').pop()?.toUpperCase() || 'FILE'
}

function getContentSizeLabel(item: DisplayClipboardItem): string | null {
  if (!item.content) return null
  switch (item.type) {
    case 'text': {
      const text = (item.content as ClipboardTextItem).display_text
      return `${text.length} chars`
    }
    case 'code': {
      const code = (item.content as ClipboardCodeItem).code
      return `${code.length} chars`
    }
    case 'link': {
      const link = item.content as ClipboardLinkItem
      return link.domains[0] ?? null
    }
    case 'image': {
      const img = item.content as ClipboardImageItem
      if (img.width > 0 && img.height > 0) return `${img.width}×${img.height}`
      if (img.size > 0) return formatFileSize(img.size)
      return null
    }
    case 'file': {
      const file = item.content as ClipboardFileItem
      const count = file.file_names.length
      const totalSize = file.file_sizes.filter(s => s >= 0).reduce((a, b) => a + b, 0)
      const sizeStr = totalSize > 0 ? formatFileSize(totalSize) : null
      if (count > 1) return sizeStr ? `${count} files` : `${count} files`
      return sizeStr
    }
    default:
      return null
  }
}

type SyncSummary = 'synced' | 'syncing' | 'partial' | 'failed' | 'pending'

function summarizeDelivery(targets: readonly EntryDeliveryTargetView[]): SyncSummary | null {
  if (targets.length === 0) return null
  let delivered = 0
  let failed = 0
  let pending = 0
  for (const t of targets) {
    switch (t.status.tag) {
      case 'delivered':
      case 'duplicate':
        delivered += 1
        break
      case 'failed':
        failed += 1
        break
      case 'pending':
        pending += 1
        break
    }
  }
  if (failed === targets.length) return 'failed'
  if (failed > 0) return 'partial'
  if (delivered === targets.length) return 'synced'
  if (delivered > 0 && pending > 0) return 'syncing'
  return 'pending'
}

// ── Sync indicator ──────────────────────────────────────────────

const SYNC_CONFIG: Record<SyncSummary, { icon: React.ElementType; color: string; spin?: boolean }> =
  {
    synced: { icon: CheckCircle2, color: 'text-emerald-500/70' },
    syncing: { icon: LoaderCircle, color: 'text-sky-500/70', spin: true },
    partial: { icon: AlertCircle, color: 'text-amber-500/70' },
    failed: { icon: AlertCircle, color: 'text-destructive/70' },
    pending: { icon: CircleDashed, color: 'text-muted-foreground/50' },
  }

const SyncIndicator: React.FC<{ delivery: EntryDeliveryView }> = ({ delivery }) => {
  const { t } = useTranslation()
  const { source, deliveries } = delivery

  if (source.tag === 'historical') return null
  const summary = summarizeDelivery(deliveries)
  if (!summary) return null

  const cfg = SYNC_CONFIG[summary]
  const Icon = cfg.icon
  return (
    <Icon
      className={cn('size-2.5', cfg.color, cfg.spin && 'animate-spin')}
      aria-label={t(`delivery.summary.${summary}`)}
    />
  )
}

// ── Source indicator ─────────────────────────────────────────────

const SOURCE_CONFIG: Record<EntrySourceView['tag'], { icon: React.ElementType; color: string }> = {
  local: { icon: Laptop, color: 'text-muted-foreground/40' },
  remote: { icon: Cloud, color: 'text-sky-500/60' },
  historical: { icon: History, color: 'text-muted-foreground/30' },
}

const SourceIndicator: React.FC<{ source: EntrySourceView }> = ({ source }) => {
  const cfg = SOURCE_CONFIG[source.tag]
  const Icon = cfg.icon
  return <Icon className={cn('size-2.5', cfg.color)} />
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

// TODO: thumbnail endpoint has issues; using original image via resource API for now
const ImageContent: React.FC<{ item: ClipboardImageItem; entryId: string }> = ({
  item,
  entryId,
}) => {
  const [imageUrl, setImageUrl] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    getClipboardEntryResource(entryId)
      .then(resource => {
        if (cancelled || !resource) return
        setImageUrl(resolveResourceImageUrl(resource))
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [entryId])

  return (
    <div className="relative rounded-lg overflow-hidden bg-muted/20 -mx-0.5">
      {imageUrl ? (
        <img src={imageUrl} alt="" className="w-full object-contain" />
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
}

const FileContent: React.FC<{ item: ClipboardFileItem }> = ({ item }) => {
  const name = item.file_names[0] ?? 'Unknown file'
  const size = item.file_sizes[0] ?? 0
  return (
    <div className="flex items-center gap-2">
      <File className="size-4 text-muted-foreground/50 shrink-0" />
      <div className="min-w-0 flex-1">
        <div className="text-[12.5px] font-medium text-foreground/85 truncate">{name}</div>
        <div className="text-[10.5px] text-muted-foreground/60">
          {getFileExtLabel(name)} - {formatFileSize(size)}
        </div>
      </div>
    </div>
  )
}

// ── Card ────────────────────────────────────────────────────────

interface HistoryCardProps {
  item: DisplayClipboardItem
  isHovered: boolean
  copySuccess: boolean
  isDeleting: boolean
  onCopy: (id: string) => void
  onHoverChange: (id: string | null) => void
}

const HistoryCard: React.FC<HistoryCardProps> = ({
  item,
  isHovered,
  copySuccess,
  isDeleting,
  onCopy,
  onHoverChange,
}) => {
  const { t } = useTranslation()
  const color = TYPE_COLOR[item.type] ?? TYPE_COLOR.unknown
  const TypeIcon = TYPE_ICONS[item.type] ?? FileText
  const sizeLabel = useMemo(() => getContentSizeLabel(item), [item])

  const { delivery } = useEntryDelivery(item.id)

  const handleMouseEnter = useCallback(() => onHoverChange(item.id), [item.id, onHoverChange])
  const handleMouseLeave = useCallback(() => onHoverChange(null), [onHoverChange])

  const content = useMemo(() => {
    if (!item.content) return null
    switch (item.type) {
      case 'text':
        return <TextContent item={item.content as ClipboardTextItem} />
      case 'code':
        return <CodeContent item={item.content as ClipboardCodeItem} />
      case 'link':
        return <LinkContent item={item.content as ClipboardLinkItem} />
      case 'image':
        return <ImageContent item={item.content as ClipboardImageItem} entryId={item.id} />
      case 'file':
        return <FileContent item={item.content as ClipboardFileItem} />
      default:
        return item.textPreview ? (
          <div className="text-[13px] text-muted-foreground/70 line-clamp-3">
            {item.textPreview}
          </div>
        ) : null
    }
  }, [item])

  return (
    <div
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      className={cn(
        'group relative px-3.5 pt-3 pb-3 border-b-[3px] border-double transition-all duration-200',
        isDeleting
          ? 'bg-destructive/10 border-border/30 opacity-60 scale-[0.97]'
          : copySuccess
            ? 'bg-emerald-500/5 border-border/30'
            : 'border-border/30 hover:bg-muted/30'
      )}
    >
      {/* Header */}
      <div className="flex items-center gap-1.5 mb-1.5">
        <TypeIcon className="size-3 shrink-0" style={{ color }} />
        <span className="text-[10.5px] font-medium" style={{ color }}>
          {t(`history.type.${item.type}`, item.type)}
        </span>

        {sizeLabel && (
          <>
            <span className="text-[9px] text-muted-foreground/25">-</span>
            <span className="text-[10px] tabular-nums text-muted-foreground/45">{sizeLabel}</span>
          </>
        )}

        <div className="ml-auto flex items-center gap-1.5">
          {delivery && (
            <>
              <SourceIndicator source={delivery.source} />
              <SyncIndicator delivery={delivery} />
            </>
          )}
          <span className="text-[10px] text-muted-foreground/40">{item.time}</span>
        </div>
      </div>

      {content}

      {/* Copy button - visible on hover */}
      <button
        type="button"
        onClick={() => onCopy(item.id)}
        className={cn(
          'absolute top-2.5 right-2.5 flex items-center justify-center size-6 rounded-md bg-card border border-border/50 text-muted-foreground shadow-sm transition-all duration-150',
          isHovered ? 'opacity-100' : 'opacity-0'
        )}
      >
        <Copy className="size-3" />
      </button>

      {/* Keyboard hint - visible on hover */}
      {isHovered && (
        <div className="absolute bottom-1 right-2.5 flex items-center gap-1.5 text-[9px] text-muted-foreground/30">
          <kbd className="px-1 py-px rounded border border-border/30 bg-muted/30 font-mono">c</kbd>
          <span>copy</span>
          <kbd className="px-1 py-px rounded border border-border/30 bg-muted/30 font-mono">d</kbd>
          <span>delete</span>
        </div>
      )}
    </div>
  )
}

export default HistoryCard
