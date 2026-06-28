import {
  AlertTriangle,
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
  Star,
  Trash2,
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
import { isImageFileName } from '@/lib/clipboard-utils'
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
    case 'image':
      // Resolution is shown in the image card's title now, not the header aux.
      return null
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

// Resolve an entry's source into a small icon + optional device label for the
// card header. `local` reads "本机"; `remote` shows the peer's device name
// (falling back to a truncated id); `historical` carries an icon only.
function describeSource(
  source: EntrySourceView,
  t: (key: string) => string
): { Icon: React.ElementType; color: string; label: string | null } {
  const cfg = SOURCE_CONFIG[source.tag]
  if (source.tag === 'remote') {
    return {
      Icon: cfg.icon,
      color: cfg.color,
      label: source.deviceName ?? source.deviceId.slice(0, 6),
    }
  }
  if (source.tag === 'local') {
    return { Icon: cfg.icon, color: cfg.color, label: t('clipboard.source.local') }
  }
  return { Icon: cfg.icon, color: cfg.color, label: null }
}

// ── Content renderers ───────────────────────────────────────────

const TextContent: React.FC<{ item: ClipboardTextItem }> = ({ item }) => {
  const isMasked = /^[•·*]{6,}$/.test(item.display_text.trim())
  return (
    <div className="text-[13px] leading-[1.55] text-foreground/85 line-clamp-2">
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

// ── Code content ────────────────────────────────────────────────
//
// A code entry keeps the shared card frame (header + theme `bg-card`, no editor
// chrome), but its body is rendered as code: a line-number gutter plus light,
// theme-aware syntax tinting. The gutter alone reads as "this is code"; the
// tint just adds depth without a hard surface boundary.

const CODE_PREVIEW_LINES = 3

// Keywords shared across the languages we're likely to see on a clipboard. The
// tint is decorative, so an over-broad set (a Python `def` highlighted in a JS
// snippet) is harmless; the goal is "this reads as code", not a real grammar.
const CODE_KEYWORDS = new Set([
  'abstract',
  'as',
  'async',
  'await',
  'break',
  'case',
  'catch',
  'class',
  'const',
  'continue',
  'def',
  'default',
  'do',
  'elif',
  'else',
  'enum',
  'export',
  'extends',
  'false',
  'final',
  'finally',
  'fn',
  'for',
  'from',
  'func',
  'function',
  'if',
  'impl',
  'import',
  'in',
  'interface',
  'let',
  'match',
  'mut',
  'new',
  'nil',
  'none',
  'null',
  'package',
  'pass',
  'private',
  'protected',
  'pub',
  'public',
  'return',
  'self',
  'static',
  'struct',
  'super',
  'switch',
  'this',
  'throw',
  'trait',
  'true',
  'try',
  'type',
  'typeof',
  'undefined',
  'use',
  'val',
  'var',
  'void',
  'where',
  'while',
  'with',
  'yield',
])

type CodeTone = 'comment' | 'string' | 'number' | 'keyword'

interface CodeSeg {
  text: string
  tone?: CodeTone
}

// Theme-aware tints: a deeper hue in light mode, a brighter one in dark, so the
// code stays legible on `bg-card` either way. Comments reuse the semantic muted
// token; the keyword violet echoes the `code` type color (rgb(140,120,210)).
const TONE_CLASS: Record<CodeTone, string> = {
  comment: 'text-muted-foreground/50 italic',
  string: 'text-emerald-600 dark:text-emerald-400',
  number: 'text-amber-600 dark:text-amber-400',
  keyword: 'text-violet-600 dark:text-violet-400',
}

// Lines whose first non-space run is a comment opener are tinted whole. `#`/`--`
// require a trailing space so CSS ids and decrement operators aren't mistaken
// for comments; `*` catches block-comment continuation lines.
const FULL_LINE_COMMENT_RE = /^(?:\/\/|#\s|--\s|\*|<!--)/

// One ordered alternation, scanned left-to-right: inline comment, then string,
// then number, then identifier. Leftmost-match semantics mean a `//` inside a
// string is consumed by the string rule (it starts earlier), so we never mistint
// `"http://"`. Block-comment state isn't carried across lines — the preview is
// line-sliced and tinting is decorative, so an unclosed `/*` only tints its own
// line.
const CODE_TOKEN_RE =
  /(\/\/.*$|\/\*.*?(?:\*\/|$))|("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`(?:[^`\\]|\\.)*`)|(\b\d[\w.]*)|([A-Za-z_$][\w$]*)/g

function tokenizeCodeLine(line: string): CodeSeg[] {
  if (FULL_LINE_COMMENT_RE.test(line.trimStart())) {
    return [{ text: line, tone: 'comment' }]
  }
  const segs: CodeSeg[] = []
  let last = 0
  CODE_TOKEN_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = CODE_TOKEN_RE.exec(line)) !== null) {
    if (m.index > last) segs.push({ text: line.slice(last, m.index) })
    if (m[1]) segs.push({ text: m[1], tone: 'comment' })
    else if (m[2]) segs.push({ text: m[2], tone: 'string' })
    else if (m[3]) segs.push({ text: m[3], tone: 'number' })
    else segs.push(CODE_KEYWORDS.has(m[4]) ? { text: m[4], tone: 'keyword' } : { text: m[4] })
    last = m.index + m[0].length
  }
  if (last < line.length) segs.push({ text: line.slice(last) })
  return segs
}

// Best-effort language label shown in the card header (replacing the generic
// "code" label). We persist only the raw code string, so infer from a few
// signature patterns and return null when nothing matches confidently — the
// header then falls back to the localized type label. Order is significant:
// more specific signatures are tested first.
function detectCodeLanguage(code: string): string | null {
  const s = code.slice(0, 1500)
  const has = (re: RegExp) => re.test(s)
  if (has(/^\s*<\?php/)) return 'PHP'
  if (has(/^#!\s*\/.*\b(?:bash|zsh|sh)\b/m) || has(/\b(?:fi|esac|elif)\b/)) return 'Shell'
  if (has(/\bfn\s+\w+/) && has(/\b(?:let\s+mut|impl|pub\s+fn|->\s*\w)/)) return 'Rust'
  if (has(/\bfunc\s+\w+/) && has(/\bpackage\s+\w+/)) return 'Go'
  if (has(/\bdef\s+\w+\s*\(/) || has(/^\s*(?:from\s+\w+\s+import|import\s+\w+)/m)) return 'Python'
  if (has(/:\s*(?:string|number|boolean|void|unknown|any)\b/) || has(/\binterface\s+\w+/)) {
    return 'TypeScript'
  }
  if (has(/\b(?:public|private|protected)\s+(?:static\s+)?(?:class|void|int|String)\b/))
    return 'Java'
  if (has(/#include\s*[<"]/)) return 'C++'
  if (has(/^\s*(?:SELECT|INSERT\s+INTO|UPDATE|DELETE\s+FROM|CREATE\s+TABLE)\b/im)) return 'SQL'
  if (has(/^\s*[{[]/) && has(/"\w+"\s*:/) && !has(/\bfunction\b|=>/)) return 'JSON'
  if (has(/<\/[a-z][\w-]*>/i) && has(/<[a-z][\w-]*[\s/>]/i)) return 'HTML'
  if (has(/[.#]?[\w-]+\s*\{[^}]*:[^}]*;/)) return 'CSS'
  if (has(/=>/) || has(/\b(?:const|let|var|function)\b/)) return 'JavaScript'
  return null
}

// Code body for the shared card frame: a line-number gutter beside theme-tinted
// code. No background block or divider — it sits directly on `bg-card` under the
// standard header, so there's no header/body seam. Long lines clip (no wrap) and
// the body is clipped to `CODE_PREVIEW_LINES` like the text card's line-clamp.
const CodeContent: React.FC<{ item: ClipboardCodeItem }> = ({ item }) => {
  const rows = useMemo(() => {
    const trimmed = item.code.replace(/\s+$/, '')
    const allLines = trimmed.length === 0 ? [''] : trimmed.split('\n')
    return allLines
      .slice(0, CODE_PREVIEW_LINES)
      .map((line, i) => ({ num: i + 1, segs: tokenizeCodeLine(line) }))
  }, [item.code])

  return (
    <div className="flex h-full font-mono text-[11px] leading-[1.55]">
      <div className="shrink-0 select-none pr-2.5 text-right tabular-nums text-muted-foreground/25">
        {rows.map(row => (
          <div key={`ln-${row.num}`}>{row.num}</div>
        ))}
      </div>
      <div className="min-w-0 flex-1 overflow-hidden">
        {rows.map(row => (
          <div key={`cl-${row.num}`} className="overflow-hidden whitespace-pre text-foreground/85">
            {row.segs.length === 0
              ? ' '
              : row.segs.map((seg, j) => (
                  <span
                    key={`s-${row.num}-${j}`}
                    className={seg.tone ? TONE_CLASS[seg.tone] : undefined}
                  >
                    {seg.text}
                  </span>
                ))}
          </div>
        ))}
      </div>
    </div>
  )
}

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

// Image body for the shared card frame: a small thumbnail (left) beside a title,
// mirroring the file-card composition. A pure bitmap has no filename, so the
// title is the localized type label with the pixel resolution in parentheses —
// e.g. "图片 (1920×1080)". The thumbnail is fetched by entry id, so search/filter
// rows that carry no structured content still preview (resolution then omitted).
const ImageContent: React.FC<{ entryId: string; imageItem?: ClipboardImageItem | null }> = ({
  entryId,
  imageItem,
}) => {
  const { t } = useTranslation()
  const imageUrl = useResourceImageUrl(entryId)
  // The browse projection often omits pixel dimensions, so fall back to reading
  // them off the loaded thumbnail (naturalWidth/Height) — same approach as the
  // detail preview.
  const [loadedDims, setLoadedDims] = useState<{ w: number; h: number } | null>(null)
  const label = t('history.type.image', 'image')
  const width = loadedDims?.w ?? (imageItem && imageItem.width > 0 ? imageItem.width : 0)
  const height = loadedDims?.h ?? (imageItem && imageItem.height > 0 ? imageItem.height : 0)
  const title = width > 0 && height > 0 ? `${label} (${width}×${height})` : label

  return (
    <div className="flex h-full items-center gap-3">
      {imageUrl ? (
        <img
          src={imageUrl}
          alt=""
          onLoad={e =>
            setLoadedDims({
              w: e.currentTarget.naturalWidth,
              h: e.currentTarget.naturalHeight,
            })
          }
          className="size-12 shrink-0 rounded-md object-cover ring-1 ring-black/5 dark:ring-white/10"
        />
      ) : (
        <div className="flex size-12 shrink-0 items-center justify-center rounded-md bg-muted/30">
          <ImageIcon className="size-5 text-muted-foreground/30" />
        </div>
      )}
      <div className="min-w-0 flex-1">
        <div className="line-clamp-2 break-all text-[13px] font-medium leading-snug text-foreground/85">
          {title}
        </div>
      </div>
    </div>
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

// A single image file (faithful `content_type=File`, but the file IS an image)
// renders with a real thumbnail in place of the lettered glyph — a preview reads
// better than a "PNG" tile. The thumbnail is fetched by entry id; the daemon
// serves the image representation's bytes (see GetEntryResourceUseCase). Falls
// back to the file glyph until the image resolves (or if it can't).
const ImageFileContent: React.FC<{ item: ClipboardFileItem; entryId: string }> = ({
  item,
  entryId,
}) => {
  const imageUrl = useResourceImageUrl(entryId)
  const name = item.file_names[0] ?? ''
  const primarySize = item.file_sizes[0] ?? -1

  return (
    <div className="flex h-full items-center gap-3">
      {imageUrl ? (
        <img
          src={imageUrl}
          alt=""
          className="size-12 shrink-0 rounded-md object-cover ring-1 ring-black/5 dark:ring-white/10"
        />
      ) : (
        <FileGlyph ext={getFileExtLabel(name)} />
      )}
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium leading-snug text-foreground/85 line-clamp-2 break-all">
          {name}
        </div>
        {primarySize >= 0 && (
          <div className="mt-1 text-[11px] tabular-nums text-muted-foreground/55">
            {formatFileSize(primarySize)}
          </div>
        )}
      </div>
    </div>
  )
}

/** A file entry that is exactly one image file — the case that renders a card thumbnail. */
function isSingleImageFile(item: ClipboardFileItem): boolean {
  return item.file_names.length === 1 && isImageFileName(item.file_names[0] ?? '')
}

// ── Card ────────────────────────────────────────────────────────

interface HistoryCardProps {
  item: DisplayClipboardItem
  isHovered: boolean
  copySuccess: boolean
  isDeleting: boolean
  onCopy: (id: string) => void
  onDelete: (id: string) => void
  onToggleFavorite: (id: string, current: boolean) => void
  onClick: (id: string) => void
  onHoverChange: (id: string | null) => void
}

const HistoryCard: React.FC<HistoryCardProps> = ({
  item,
  isHovered,
  copySuccess,
  isDeleting,
  onCopy,
  onDelete,
  onToggleFavorite,
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
  const isFavorited = item.isFavorited ?? false
  const isUnavailable = item.isUnavailable ?? false
  // Code keeps the shared card frame; only its header label swaps the generic
  // "code" for an inferred language (when detectable), and its body renders as
  // line-numbered, tinted code via CodeContent.
  const codeLanguage = useMemo(
    () =>
      item.type === 'code'
        ? detectCodeLanguage(
            (item.content as ClipboardCodeItem | null)?.code ?? item.textPreview ?? ''
          )
        : null,
    [item]
  )
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
    // Image previews are fetched by entry id, so they render the same way whether
    // or not structured content is present (search rows carry none).
    if (item.type === 'image') {
      return (
        <ImageContent entryId={item.id} imageItem={item.content as ClipboardImageItem | null} />
      )
    }
    if (!item.content) {
      // File-type search rows carry no structured content (the search index drops
      // file_names/sizes), so synthesize a minimal file item from the preview —
      // a filtered file then renders as a file card, not a raw path/URL line.
      // Size and file count stay unknown in search mode.
      if (item.type === 'file' && item.textPreview) {
        const fileItem: ClipboardFileItem = {
          file_names: [fileNameFromPreview(item.textPreview)],
          file_sizes: [-1],
        }
        return isSingleImageFile(fileItem) ? (
          <ImageFileContent item={fileItem} entryId={item.id} />
        ) : (
          <FileContent item={fileItem} />
        )
      }
      // Code-type search rows keep the code treatment, synthesizing a code item
      // from the preview (the search index drops structured content too).
      if (item.type === 'code' && item.textPreview) {
        return <CodeContent item={{ code: item.textPreview }} />
      }
      // Other search/pending rows carry only a text preview; render it as a plain
      // snippet so search hits aren't shown as blank cards.
      return item.textPreview ? (
        <div className="text-[13px] leading-[1.55] text-foreground/85 line-clamp-2 break-words whitespace-pre-wrap">
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
      case 'file': {
        const fileItem = item.content as ClipboardFileItem
        return isSingleImageFile(fileItem) ? (
          <ImageFileContent item={fileItem} entryId={item.id} />
        ) : (
          <FileContent item={fileItem} />
        )
      }
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

  // Source meta (icon + device label) for the header's right cluster.
  const source = delivery ? describeSource(delivery.source, t) : null

  const actionBtnClass =
    'flex size-6 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-foreground/10 hover:text-foreground'

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
        'group relative flex h-full cursor-pointer flex-col overflow-hidden px-3.5 py-2.5 transition-all duration-200',
        isDeleting
          ? 'bg-destructive/10 opacity-60 scale-[0.97]'
          : copySuccess
            ? 'bg-emerald-500/5'
            : isPending
              ? 'bg-muted/10'
              : 'hover:bg-muted/40',
        isUnavailable && 'opacity-55'
      )}
    >
      {/* Transfer progress overlay — card acts as an immersive progress bar */}
      {isFileType && (
        <div
          className={cn(
            'absolute inset-0 z-0 bg-primary/8 transition-all duration-500 ease-out',
            isTransferring && transfer ? 'opacity-100' : 'opacity-0'
          )}
          style={{ width: isTransferring && transfer ? `${percent}%` : '100%' }}
        />
      )}

      {/* ── Meta layer: type · aux (left) · source / time / status (right) ── */}
      <div className="relative z-10 mb-1.5 flex items-center gap-1.5">
        <TypeIcon className={cn('size-3 shrink-0', isPending && 'opacity-50')} style={{ color }} />
        <span
          className={cn('text-[10.5px] font-medium', isPending && 'opacity-50')}
          style={{ color }}
        >
          {codeLanguage ?? t(`history.type.${item.type}`, item.type)}
        </span>

        {sizeLabel && !isTransferring && (
          <>
            <span className="text-[9px] text-muted-foreground/25">·</span>
            <span className="truncate text-[10px] tabular-nums text-muted-foreground/45">
              {sizeLabel}
            </span>
          </>
        )}

        <div className="ml-auto flex shrink-0 items-center gap-1.5">
          {isUnavailable && (
            <AlertTriangle
              className="size-2.5 text-amber-500/70"
              aria-label={t('clipboard.errors.unavailableBadge')}
            />
          )}
          {isFavorited && <Star className="size-2.5 fill-amber-400 text-amber-400" />}
          {isFileType && isTransferring ? (
            <>
              <DirectionIcon className="size-2.5 text-primary/70" />
              <span className="text-[10px] font-medium tabular-nums text-primary/80">
                {percent}%
              </span>
              {speedLabel && (
                <>
                  <span className="text-[9px] text-primary/30">·</span>
                  <span className="text-[10px] tabular-nums text-primary/70">{speedLabel}</span>
                </>
              )}
            </>
          ) : isFileType && isPending ? (
            <>
              <LoaderCircle className="size-2.5 animate-spin text-muted-foreground/40" />
              <span className="text-[10px] text-muted-foreground/40">
                {t('clipboard.transfer.pending')}
              </span>
            </>
          ) : (
            <span className="flex items-center gap-1 text-[10px] text-muted-foreground/45">
              {source?.Icon && <source.Icon className={cn('size-2.5', source.color)} />}
              {source?.label && (
                <>
                  <span className="max-w-[7rem] truncate">{source.label}</span>
                  <span className="text-muted-foreground/25">·</span>
                </>
              )}
              <span className="tabular-nums">{relativeTime}</span>
            </span>
          )}
        </div>
      </div>

      {/* ── Content layer ── */}
      <div
        className={cn('relative z-10 min-h-0 flex-1 overflow-hidden', isPending && 'opacity-60')}
      >
        {content}
      </div>

      {/* Transfer progress detail bar — absolute so it never affects card height */}
      {isFileType && (
        <div
          className={cn(
            'absolute bottom-1.5 left-3.5 right-3.5 z-10 flex items-center gap-1.5 transition-opacity duration-500 ease-out',
            isTransferring && transfer ? 'opacity-100' : 'opacity-0 pointer-events-none'
          )}
        >
          {transfer && (
            <>
              <div className="h-px flex-1 overflow-hidden rounded-full bg-primary/15">
                <div
                  className="h-full bg-primary/40 transition-[width] duration-300 ease-out"
                  style={{ width: `${percent}%` }}
                />
              </div>
              <span className="shrink-0 text-[9px] tabular-nums text-primary/50">
                {transfer.totalBytes
                  ? `${formatFileSize(transfer.bytesTransferred)} / ${formatFileSize(transfer.totalBytes)}`
                  : formatFileSize(transfer.bytesTransferred)}
              </span>
            </>
          )}
        </div>
      )}

      {/* ── Action layer (hover): copy / favorite / delete as one aligned group ── */}
      <div
        className={cn(
          'absolute bottom-1.5 right-2 z-20 flex items-center gap-0.5 rounded-lg border border-border/40 bg-card/95 p-0.5 shadow-sm backdrop-blur transition-opacity duration-150',
          isHovered && !isTransferring && !isPending
            ? 'opacity-100'
            : 'pointer-events-none opacity-0'
        )}
      >
        <button
          type="button"
          aria-label={t('clipboard.item.actions.copy')}
          tabIndex={isHovered ? 0 : -1}
          onClick={e => {
            e.stopPropagation()
            onCopy(item.id)
          }}
          className={actionBtnClass}
        >
          <Copy className="size-3" />
        </button>
        <button
          type="button"
          aria-label={t(
            isFavorited ? 'clipboard.item.actions.unfavorite' : 'clipboard.item.actions.favorite'
          )}
          tabIndex={isHovered ? 0 : -1}
          onClick={e => {
            e.stopPropagation()
            onToggleFavorite(item.id, isFavorited)
          }}
          className={actionBtnClass}
        >
          <Star className={cn('size-3', isFavorited && 'fill-amber-400 text-amber-400')} />
        </button>
        <button
          type="button"
          aria-label={t('clipboard.item.actions.delete')}
          tabIndex={isHovered ? 0 : -1}
          onClick={e => {
            e.stopPropagation()
            onDelete(item.id)
          }}
          className={cn(actionBtnClass, 'hover:bg-destructive/10 hover:text-destructive')}
        >
          <Trash2 className="size-3" />
        </button>
      </div>
    </div>
  )
}

export default HistoryCard
