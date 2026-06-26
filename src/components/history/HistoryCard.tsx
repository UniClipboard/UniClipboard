import {
  ArrowDownToLine,
  ArrowUpFromLine,
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
} from 'lucide-react'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { resolveResourceImageUrl } from '@/api/clipboardItems'
import { getClipboardEntryResource } from '@/api/daemon/clipboard'
import type { EntrySourceView } from '@/api/tauri-command/clipboard_delivery'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import { useRelativeTime } from '@/hooks/useRelativeTime'
import type {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'
import { useAppSelector } from '@/store/hooks'
import {
  resolveEntryTransferStatus,
  selectEntryTransferStatus,
  selectTransferByEntryId,
} from '@/store/slices/fileTransferSlice'
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

// Reduce a file entry's preview string (a bare file name, a native path, or a
// `file://` URL) to its display file name: the last path segment, percent-decoded.
// Search rows carry no structured file_names, so this recovers a name from the
// preview the search index does keep.
function fileNameFromPreview(preview: string): string {
  const trimmed = preview.trim().replace(/[/\\]+$/, '')
  const segment = trimmed.split(/[/\\]/).pop() ?? trimmed
  try {
    return decodeURIComponent(segment)
  } catch {
    return segment
  }
}

function getContentSizeLabel(
  item: DisplayClipboardItem,
  t: (key: string, opts?: Record<string, unknown>) => string
): string | null {
  if (!item.content) return null
  switch (item.type) {
    case 'text': {
      const text = (item.content as ClipboardTextItem).display_text
      return t('clipboard.preview.charactersCount', { count: text.length })
    }
    case 'code': {
      const code = (item.content as ClipboardCodeItem).code
      return t('clipboard.preview.charactersCount', { count: code.length })
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
      if (count > 1) return t('clipboard.preview.filesCount', { count })
      const totalSize = file.file_sizes.filter(s => s >= 0).reduce((a, b) => a + b, 0)
      return totalSize > 0 ? formatFileSize(totalSize) : null
    }
    default:
      return null
  }
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

// Module-level cache of resolved image URLs, keyed by entryId. Survives card
// remounts (e.g. when a new item shifts every card to a different column),
// so the image initializes synchronously instead of flashing the placeholder
// and re-fetching.
// `null` is a real, cached value: it records an entry that resolved to no image
// so the hook stops re-fetching it on every card remount. Only deterministic
// "no resource / unresolvable" outcomes are cached; thrown errors are not, so a
// transient daemon hiccup can still be retried.
const imageUrlCache = new Map<string, string | null>()

// TODO: thumbnail endpoint has issues; using original image via resource API for now
function useResourceImageUrl(entryId: string): string | null {
  const [imageUrl, setImageUrl] = useState<string | null>(() => imageUrlCache.get(entryId) ?? null)

  useEffect(() => {
    if (imageUrlCache.has(entryId)) {
      setImageUrl(imageUrlCache.get(entryId) ?? null)
      return
    }
    let cancelled = false
    getClipboardEntryResource(entryId)
      .then(resource => {
        if (cancelled) return
        const url = resource ? resolveResourceImageUrl(resource) : null
        imageUrlCache.set(entryId, url)
        setImageUrl(url)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [entryId])

  return imageUrl
}

// Immersive image card: the image fills the whole card as a background, with the
// metadata (type, time) and pixel dimensions floated on top of legibility
// gradients. Distinct from the header+content stack used by every other type.
const ImageCard: React.FC<{
  item: DisplayClipboardItem
  // Optional: search/filter rows carry no structured content. The thumbnail is
  // fetched by entry id, so it's only the pixel-size badge that needs this.
  imageItem?: ClipboardImageItem | null
}> = ({ item, imageItem }) => {
  const { t } = useTranslation()
  const imageUrl = useResourceImageUrl(item.id)
  const relativeTime = useRelativeTime(item.activeTime)

  return (
    <>
      {imageUrl ? (
        <img src={imageUrl} alt="" className="absolute inset-0 size-full object-cover" />
      ) : (
        <div className="absolute inset-0 flex items-center justify-center bg-muted/30">
          <ImageIcon className="size-8 text-muted-foreground/25" />
        </div>
      )}

      {/* Legibility gradients behind the overlaid text */}
      <div className="pointer-events-none absolute inset-x-0 top-0 h-16 bg-gradient-to-b from-black/55 to-transparent" />
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-16 bg-gradient-to-t from-black/55 to-transparent" />

      {/* Floated metadata header */}
      <div className="absolute inset-x-3.5 top-3 z-10 flex items-center gap-1.5 drop-shadow-[0_1px_2px_rgba(0,0,0,0.55)]">
        <ImageIcon className="size-3 shrink-0 text-white/90" />
        <span className="text-[10.5px] font-medium text-white/90">
          {t('history.type.image', 'image')}
        </span>
        <span className="ml-auto text-[10px] text-white/75">{relativeTime}</span>
      </div>

      {/* Pixel dimensions badge */}
      {imageItem && imageItem.width > 0 && imageItem.height > 0 && (
        <span className="absolute bottom-3 left-3.5 z-10 text-[11px] font-medium tabular-nums text-white/85 drop-shadow-[0_1px_3px_rgba(0,0,0,0.6)]">
          {imageItem.width}×{imageItem.height}
        </span>
      )}
    </>
  )
}

// Per-type colors for the file glyph, so a file reads as "a PDF / ZIP / image"
// from color alone — the strongest at-a-glance recognition cue (see DailyUI
// file-upload patterns). Mid-tone fills keep white extension text legible.
const FILE_TYPE_COLORS: { exts: string[]; color: string }[] = [
  { exts: ['PDF'], color: 'rgb(212,88,82)' },
  { exts: ['DOC', 'DOCX', 'RTF', 'TXT', 'MD', 'PAGES'], color: 'rgb(72,118,196)' },
  { exts: ['XLS', 'XLSX', 'CSV', 'NUMBERS'], color: 'rgb(58,158,108)' },
  { exts: ['PPT', 'PPTX', 'KEY'], color: 'rgb(218,138,72)' },
  { exts: ['ZIP', 'RAR', '7Z', 'GZ', 'TAR'], color: 'rgb(176,142,96)' },
  { exts: ['PNG', 'JPG', 'JPEG', 'GIF', 'SVG', 'WEBP', 'HEIC', 'BMP'], color: 'rgb(150,112,202)' },
  { exts: ['MP4', 'MOV', 'AVI', 'MKV', 'WEBM'], color: 'rgb(92,120,210)' },
  { exts: ['MP3', 'WAV', 'FLAC', 'AAC', 'M4A'], color: 'rgb(202,100,150)' },
  // prettier-ignore
  { exts: ['JS', 'TS', 'TSX', 'JSX', 'PY', 'RS', 'GO', 'JSON', 'HTML', 'CSS', 'SH'], color: 'rgb(110,120,136)' },
]

// Flatten the ext→color groups into a single lookup map so resolving a file's
// color is an O(1) Map.get instead of scanning every group's `exts` per call.
const EXT_COLOR = new Map<string, string>(
  FILE_TYPE_COLORS.flatMap(group => group.exts.map(ext => [ext, group.color] as const))
)

function fileTypeColor(ext: string): string {
  return EXT_COLOR.get(ext.toUpperCase()) ?? 'rgb(140,150,160)'
}

// A document-shaped, color-coded tile with the extension lettered in — the
// canonical "file" representation (folded corner + type color + extension).
const FileGlyph: React.FC<{ ext: string; stacked?: boolean }> = ({ ext, stacked }) => {
  const color = fileTypeColor(ext)
  const label = ext.length > 4 ? ext.slice(0, 4) : ext
  return (
    <div className="relative shrink-0">
      {/* Stacked-sheet hint for multi-file entries */}
      {stacked && (
        <div
          aria-hidden
          className="absolute -right-1 -top-1 h-12 w-10 rounded-md bg-muted-foreground/25"
        />
      )}
      <div
        className="relative flex h-12 w-10 items-center justify-center overflow-hidden rounded-md"
        style={{ backgroundColor: color }}
      >
        {/* Folded top-right corner */}
        <div className="absolute right-0 top-0 size-3 rounded-bl-md bg-black/20" />
        <span className="px-0.5 text-[9px] font-bold uppercase tracking-wide text-white">
          {label}
        </span>
      </div>
    </div>
  )
}

// File card body: a color-coded file glyph (left) anchors recognition, with the
// name + size beside it — the standard, scannable file list-item composition.
const FileContent: React.FC<{ item: ClipboardFileItem }> = ({ item }) => {
  const { t } = useTranslation()
  const count = item.file_names.length
  const name = item.file_names[0] ?? t('history.unknownFile')
  const primarySize = item.file_sizes[0] ?? -1
  const ext = getFileExtLabel(name)
  const totalSize = item.file_sizes.filter(s => s >= 0).reduce((a, b) => a + b, 0)

  // Extension lives on the glyph, so meta only adds size / file count.
  const meta =
    count > 1
      ? totalSize > 0
        ? `${t('clipboard.preview.filesCount', { count })} · ${formatFileSize(totalSize)}`
        : t('clipboard.preview.filesCount', { count })
      : primarySize >= 0
        ? formatFileSize(primarySize)
        : ''

  return (
    <div className="flex h-full items-center gap-3">
      <FileGlyph ext={ext} stacked={count > 1} />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium leading-snug text-foreground/85 line-clamp-2 break-all">
          {name}
        </div>
        {meta && (
          <div className="mt-1 text-[11px] tabular-nums text-muted-foreground/55">{meta}</div>
        )}
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
  onClick: (id: string) => void
  onHoverChange: (id: string | null) => void
}

const HistoryCard: React.FC<HistoryCardProps> = ({
  item,
  isHovered,
  copySuccess,
  isDeleting,
  onCopy,
  onClick,
  onHoverChange,
}) => {
  const { t } = useTranslation()
  const relativeTime = useRelativeTime(item.activeTime)
  const color = TYPE_COLOR[item.type] ?? TYPE_COLOR.unknown
  const TypeIcon = TYPE_ICONS[item.type] ?? FileText
  const sizeLabel = useMemo(() => getContentSizeLabel(item, t), [item, t])

  const { delivery } = useEntryDelivery(item.id)

  const isFileType = item.type === 'file'
  // Every image entry renders as an immersive full-bleed card. The thumbnail is
  // fetched by entry id (see useResourceImageUrl), so search/filter rows that
  // carry no structured content still show the image — only the pixel-size badge
  // (which needs content) is omitted.
  const isImageCard = item.type === 'image'
  const transfer = useAppSelector(state =>
    isFileType ? selectTransferByEntryId(state, item.id) : undefined
  )
  const entryStatus = useAppSelector(state =>
    isFileType ? selectEntryTransferStatus(state, item.id) : undefined
  )
  const effectiveStatus = isFileType ? resolveEntryTransferStatus(entryStatus, transfer) : undefined

  const isTransferring = effectiveStatus === 'transferring'
  const isPending = effectiveStatus === 'pending'

  const percent =
    transfer && transfer.totalBytes && transfer.totalBytes > 0
      ? Math.round((transfer.bytesTransferred / transfer.totalBytes) * 100)
      : 0

  const speedLabel = transfer?.bytesPerSecond
    ? formatFileSize(transfer.bytesPerSecond) + '/s'
    : null

  const handleMouseEnter = useCallback(() => onHoverChange(item.id), [item.id, onHoverChange])
  const handleMouseLeave = useCallback(() => onHoverChange(null), [onHoverChange])

  const content = useMemo(() => {
    if (!item.content) {
      // File-type search rows carry no structured content (the search index drops
      // file_names/sizes), so synthesize a minimal file item from the preview —
      // a filtered file then renders as a file card, not a raw path/URL line.
      // Size and file count stay unknown in search mode.
      if (item.type === 'file' && item.textPreview) {
        return (
          <FileContent
            item={{ file_names: [fileNameFromPreview(item.textPreview)], file_sizes: [-1] }}
          />
        )
      }
      // Other search/pending rows carry only a text preview; render it as a plain
      // snippet so search hits aren't shown as blank cards.
      return item.textPreview ? (
        <div className="text-[13px] leading-[1.55] text-foreground/85 line-clamp-4 break-words whitespace-pre-wrap">
          {item.textPreview}
        </div>
      ) : null
    }
    switch (item.type) {
      case 'text':
        return <TextContent item={item.content as ClipboardTextItem} />
      case 'code':
        return <CodeContent item={item.content as ClipboardCodeItem} />
      case 'link':
        return <LinkContent item={item.content as ClipboardLinkItem} />
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

  const handleClick = useCallback(() => onClick(item.id), [item.id, onClick])

  const DirectionIcon = transfer?.direction === 'Sending' ? ArrowUpFromLine : ArrowDownToLine

  // Keyboard-hint chip styling: dark chips + light borders read over a photo,
  // muted chips suit the opaque card surface of every other type.
  const kbdClass = isImageCard
    ? 'border-white/25 bg-black/45 text-white/90'
    : 'border-border/30 bg-muted/30'

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={handleClick}
      onKeyDown={e => {
        if (e.key === 'Enter' || e.key === ' ') handleClick()
      }}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      className={cn(
        'cursor-pointer overflow-hidden h-full group relative transition-all duration-200',
        isImageCard ? '' : 'flex flex-col px-3.5 pt-3 pb-3',
        isDeleting
          ? 'bg-destructive/10 opacity-60 scale-[0.97]'
          : copySuccess
            ? 'bg-emerald-500/5'
            : isPending
              ? 'bg-muted/10'
              : 'hover:bg-muted/30'
      )}
    >
      {/* Transfer progress overlay - card acts as an immersive progress bar */}
      {isFileType && (
        <div
          className={cn(
            'absolute inset-0 z-0 bg-primary/8 transition-all duration-500 ease-out',
            isTransferring && transfer ? 'opacity-100' : 'opacity-0'
          )}
          style={{ width: isTransferring && transfer ? `${percent}%` : '100%' }}
        />
      )}

      {isImageCard && (
        <ImageCard item={item} imageItem={item.content as ClipboardImageItem | null} />
      )}

      {!isImageCard && (
        <>
          {/* Header */}
          <div className="relative z-10 flex items-center gap-1.5 mb-1.5">
            <TypeIcon
              className={cn('size-3 shrink-0', isPending && 'opacity-50')}
              style={{ color }}
            />
            <span
              className={cn('text-[10.5px] font-medium', isPending && 'opacity-50')}
              style={{ color }}
            >
              {t(`history.type.${item.type}`, item.type)}
            </span>

            {sizeLabel && !isTransferring && (
              <>
                <span className="text-[9px] text-muted-foreground/25">-</span>
                <span className="text-[10px] tabular-nums text-muted-foreground/45">
                  {sizeLabel}
                </span>
              </>
            )}

            <div className="ml-auto flex items-center gap-1.5">
              {isFileType && isTransferring ? (
                <>
                  <DirectionIcon className="size-2.5 text-primary/70" />
                  <span className="text-[10px] tabular-nums text-primary/80 font-medium">
                    {percent}%
                  </span>
                  {speedLabel && (
                    <>
                      <span className="text-[9px] text-primary/30">-</span>
                      <span className="text-[10px] tabular-nums text-primary/70">{speedLabel}</span>
                    </>
                  )}
                </>
              ) : isFileType && isPending ? (
                <>
                  <LoaderCircle className="size-2.5 text-muted-foreground/40 animate-spin" />
                  <span className="text-[10px] text-muted-foreground/40">
                    {t('clipboard.transfer.pending')}
                  </span>
                </>
              ) : (
                <>
                  {delivery && <SourceIndicator source={delivery.source} />}
                  <span className="text-[10px] text-muted-foreground/40">{relativeTime}</span>
                </>
              )}
            </div>
          </div>

          <div
            className={cn(
              'relative z-10 flex-1 min-h-0 overflow-hidden',
              isPending && 'opacity-60'
            )}
          >
            {content}
          </div>
        </>
      )}

      {/* Transfer progress detail bar at bottom — absolute so it never affects card height */}
      {isFileType && (
        <div
          className={cn(
            'absolute bottom-1.5 left-3.5 right-3.5 z-10 flex items-center gap-1.5 transition-opacity duration-500 ease-out',
            isTransferring && transfer ? 'opacity-100' : 'opacity-0 pointer-events-none'
          )}
        >
          {transfer && (
            <>
              <div className="h-px flex-1 bg-primary/15 rounded-full overflow-hidden">
                <div
                  className="h-full bg-primary/40 transition-[width] duration-300 ease-out"
                  style={{ width: `${percent}%` }}
                />
              </div>
              <span className="text-[9px] tabular-nums text-primary/50 shrink-0">
                {transfer.totalBytes
                  ? `${formatFileSize(transfer.bytesTransferred)} / ${formatFileSize(transfer.totalBytes)}`
                  : formatFileSize(transfer.bytesTransferred)}
              </span>
            </>
          )}
        </div>
      )}

      {/* Copy button - visible on hover, hidden during transfer */}
      <button
        type="button"
        aria-label={t('clipboard.item.actions.copy')}
        tabIndex={isHovered && !isTransferring ? 0 : -1}
        onClick={e => {
          e.stopPropagation()
          onCopy(item.id)
        }}
        className={cn(
          'absolute top-2.5 right-2.5 z-20 flex items-center justify-center size-6 rounded-md bg-card border border-border/50 text-muted-foreground shadow-sm transition-all duration-150',
          isHovered && !isTransferring ? 'opacity-100' : 'opacity-0 pointer-events-none'
        )}
      >
        <Copy className="size-3" />
      </button>

      {/* Keyboard hint - visible on hover. On image cards it sits over the photo,
          so it needs a high-contrast treatment (light text + dark chips). */}
      {isHovered && !isTransferring && !isPending && (
        <div
          className={cn(
            'absolute bottom-1 right-2.5 z-20 flex items-center gap-1.5 text-[9px]',
            isImageCard
              ? 'text-white/80 drop-shadow-[0_1px_2px_rgba(0,0,0,0.7)]'
              : 'text-muted-foreground/30'
          )}
        >
          <kbd className={cn('px-1 py-px rounded border font-mono', kbdClass)}>c</kbd>
          <span>{t('clipboard.item.actions.copy')}</span>
          <kbd className={cn('px-1 py-px rounded border font-mono', kbdClass)}>d</kbd>
          <span>{t('clipboard.item.actions.delete')}</span>
        </div>
      )}
    </div>
  )
}

export default HistoryCard
