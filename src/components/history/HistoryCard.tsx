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

// ── Code content ────────────────────────────────────────────────
//
// A code entry keeps the shared card frame (header + theme `bg-card`, no editor
// chrome), but its body is rendered as code: a line-number gutter plus light,
// theme-aware syntax tinting. The gutter alone reads as "this is code"; the
// tint just adds depth without a hard surface boundary.

const CODE_PREVIEW_LINES = 8

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
              {codeLanguage ?? t(`history.type.${item.type}`, item.type)}
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
