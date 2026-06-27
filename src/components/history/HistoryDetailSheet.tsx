import {
  Check,
  Code,
  Copy,
  ExternalLink,
  File,
  FileText,
  Image as ImageIcon,
  Trash2,
} from 'lucide-react'
import React, { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { resolveResourceImageUrl } from '@/api/clipboardItems'
import { getEntryDetail, getClipboardEntryResource } from '@/api/daemon/clipboard'
import type { EntryDetail } from '@/api/daemon/clipboard'
import EntryDeliveryBadge from '@/components/clipboard/EntryDeliveryBadge'
import { Button } from '@/components/ui/button'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import type {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { isImageFileName } from '@/lib/clipboard-utils'
import { formatFileSize } from '@/utils'

// ── Helpers ─────────────────────────────────────────────────────

const TYPE_ICONS: Record<string, React.ElementType> = {
  text: FileText,
  code: Code,
  link: ExternalLink,
  image: ImageIcon,
  file: File,
  unknown: FileText,
}

function formatAbsoluteTime(ms: number): string {
  const d = new Date(ms)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

function getFileExtLabel(name: string): string {
  return name.split('.').pop()?.toUpperCase() || 'FILE'
}

// ── Detail content renderers ────────────────────────────────────

const DetailText: React.FC<{ item: ClipboardTextItem; entryId: string }> = ({ item, entryId }) => {
  const [fullText, setFullText] = useState<string | null>(null)

  useEffect(() => {
    if (!item.has_detail) return
    let cancelled = false
    getEntryDetail(entryId)
      .then((detail: EntryDetail | null) => {
        if (cancelled || !detail) return
        setFullText(detail.content)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [entryId, item.has_detail])

  const text = fullText ?? item.display_text
  return (
    <div className="whitespace-pre-wrap break-words text-[13px] leading-relaxed text-foreground/90">
      {text}
    </div>
  )
}

const DetailCode: React.FC<{ item: ClipboardCodeItem; entryId: string }> = ({ item, entryId }) => {
  const [fullCode, setFullCode] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    getEntryDetail(entryId)
      .then((detail: EntryDetail | null) => {
        if (cancelled || !detail) return
        setFullCode(detail.content)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [entryId])

  const code = fullCode ?? item.code
  return (
    <pre className="rounded-lg bg-[#1a1726] px-4 py-3 text-[11px] leading-[1.65] text-[#c8c0e0] font-mono overflow-x-auto">
      <code>{code}</code>
    </pre>
  )
}

const DetailLink: React.FC<{ item: ClipboardLinkItem }> = ({ item }) => (
  <div className="space-y-3">
    {item.urls.map(url => (
      <div key={url} className="rounded-lg bg-muted/20 px-3 py-2.5">
        <div className="flex items-center gap-1.5 mb-1">
          <ExternalLink className="size-3 text-muted-foreground/40 shrink-0" />
          <span className="text-[10.5px] text-muted-foreground/50">URL</span>
        </div>
        <a
          href={url}
          target="_blank"
          rel="noopener noreferrer"
          className="text-[13px] leading-relaxed text-sky-500 hover:underline break-all"
        >
          {url}
        </a>
      </div>
    ))}
  </div>
)

// Resolve an entry's image bytes to a render URL by entry id. `enabled` gates
// the fetch so non-image entries don't hit the resource endpoint. The daemon
// serves the entry's image representation (a pure bitmap, or an image file's
// materialized blob) — see GetEntryResourceUseCase.
// TODO: thumbnail endpoint has issues; using original image via resource API.
function useResourceImageUrl(entryId: string, enabled: boolean): string | null {
  const [imageUrl, setImageUrl] = useState<string | null>(null)

  useEffect(() => {
    if (!enabled) return
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
  }, [entryId, enabled])

  return imageUrl
}

const DetailImage: React.FC<{ item: ClipboardImageItem; entryId: string }> = ({
  item,
  entryId,
}) => {
  const imageUrl = useResourceImageUrl(entryId, true)

  return (
    <div className="flex flex-col items-center gap-2">
      {imageUrl ? (
        <img
          src={imageUrl}
          alt=""
          className="max-w-full rounded-lg object-contain shadow-lg ring-1 ring-black/5 dark:ring-white/10"
        />
      ) : (
        <div className="h-40 w-full flex items-center justify-center rounded-lg bg-muted/20">
          <ImageIcon className="size-8 text-muted-foreground/20" />
        </div>
      )}
      {item.width > 0 && item.height > 0 && (
        <span className="text-[11px] text-muted-foreground/50">
          {item.width} x {item.height}
          {item.size > 0 && ` - ${formatFileSize(item.size)}`}
        </span>
      )}
    </div>
  )
}

const DetailFile: React.FC<{ item: ClipboardFileItem; entryId: string }> = ({ item, entryId }) => {
  const { t } = useTranslation()
  // An image file (or a multi-file selection that includes one) is physically a
  // file, but previewing the image reads better than a bare file list — fetch a
  // thumbnail when any file is an image (the daemon resolves the image rep).
  const hasImage = item.file_names.some(isImageFileName)
  const imageUrl = useResourceImageUrl(entryId, hasImage)

  return (
    <div className="space-y-2">
      {imageUrl && (
        <img
          src={imageUrl}
          alt=""
          className="mb-1 max-h-60 w-full rounded-lg object-contain bg-muted/20 ring-1 ring-black/5 dark:ring-white/10"
        />
      )}
      {item.file_names.map((name, i) => {
        const size = item.file_sizes[i] ?? 0
        const missing = item.file_missing?.[i] ?? false
        return (
          <div key={name} className="rounded-lg bg-muted/20 px-3 py-2.5">
            <div className="flex items-center gap-1.5 mb-1">
              <File className="size-3 text-muted-foreground/40 shrink-0" />
              <span className="text-[10.5px] text-muted-foreground/50">
                {getFileExtLabel(name)}
                {size > 0 && ` - ${formatFileSize(size)}`}
              </span>
              {missing && (
                <span className="text-[10px] text-destructive/70">
                  {t('history.detail.fileMissing')}
                </span>
              )}
            </div>
            <div className="text-[12.5px] font-medium text-foreground/85 break-all leading-relaxed">
              {name}
            </div>
          </div>
        )
      })}
    </div>
  )
}

// ── Meta row ────────────────────────────────────────────────────

interface MetaItem {
  label: string
  value: string
}

function buildMeta(
  item: DisplayClipboardItem,
  t: (key: string, opts?: Record<string, unknown>) => string
): MetaItem[] {
  const rows: MetaItem[] = []

  rows.push({ label: t('history.detail.capturedAt'), value: formatAbsoluteTime(item.activeTime) })

  if (!item.content) return rows

  switch (item.type) {
    case 'text': {
      const text = (item.content as ClipboardTextItem).display_text
      rows.push({
        label: t('history.detail.size'),
        value: t('clipboard.preview.charactersCount', { count: text.length }),
      })
      break
    }
    case 'code': {
      const code = (item.content as ClipboardCodeItem).code
      rows.push({
        label: t('history.detail.size'),
        value: t('clipboard.preview.charactersCount', { count: code.length }),
      })
      break
    }
    case 'image': {
      const img = item.content as ClipboardImageItem
      if (img.size > 0)
        rows.push({ label: t('history.detail.size'), value: formatFileSize(img.size) })
      break
    }
    case 'file': {
      const file = item.content as ClipboardFileItem
      const total = file.file_sizes.filter(s => s >= 0).reduce((a, b) => a + b, 0)
      rows.push({ label: t('history.detail.fileCount'), value: String(file.file_names.length) })
      if (total > 0) rows.push({ label: t('history.detail.size'), value: formatFileSize(total) })
      break
    }
    case 'link': {
      const link = item.content as ClipboardLinkItem
      rows.push({ label: t('history.detail.urlCount'), value: String(link.urls.length) })
      break
    }
  }

  return rows
}

// ── Sheet ───────────────────────────────────────────────────────

interface HistoryDetailSheetProps {
  item: DisplayClipboardItem | null
  open: boolean
  onOpenChange: (open: boolean) => void
  onCopy: (id: string) => Promise<boolean>
  onDelete: (id: string) => Promise<boolean>
}

const HistoryDetailSheet: React.FC<HistoryDetailSheetProps> = ({
  item,
  open,
  onOpenChange,
  onCopy,
  onDelete,
}) => {
  const { t } = useTranslation()
  const { delivery } = useEntryDelivery(open ? (item?.id ?? null) : null)
  const [copyDone, setCopyDone] = useState(false)

  // Push a modal layer while the sheet is open so the page-scoped grid shortcuts
  // (c copy / d delete) are suspended in favor of this sheet's own handlers.
  // Escape is handled explicitly below (capture phase) — not via this layer or
  // Radix's default — so it closes the sheet without clearing the history
  // filters.
  useShortcutLayer({ layer: 'modal', scope: 'modal', enabled: open })

  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const TypeIcon = item ? (TYPE_ICONS[item.type] ?? FileText) : FileText
  const meta = useMemo(() => (item ? buildMeta(item, t) : []), [item, t])

  const handleCopy = useCallback(async () => {
    if (!item) return
    const ok = await onCopy(item.id)
    if (ok) {
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current)
      setCopyDone(true)
      copyTimerRef.current = setTimeout(() => setCopyDone(false), 1200)
    }
  }, [item, onCopy])

  const handleDelete = useCallback(async () => {
    if (!item) return
    const ok = await onDelete(item.id)
    if (ok) onOpenChange(false)
  }, [item, onDelete, onOpenChange])

  // Reset the transient "copied" badge as part of the close event rather than a
  // prop-watching effect (avoids the no-adjust-state-on-prop-change pattern).
  const handleOpenChange = useCallback(
    (next: boolean) => {
      if (!next) setCopyDone(false)
      onOpenChange(next)
    },
    [onOpenChange]
  )

  // Read the latest copy/delete handlers via useEffectEvent so the keydown
  // listener subscribes once per open instead of re-subscribing every time the
  // handler identity changes (e.g. when the selected item switches).
  const onCopyKey = useEffectEvent(() => void handleCopy())
  const onDeleteKey = useEffectEvent(() => void handleDelete())
  const onCloseKey = useEffectEvent(() => handleOpenChange(false))

  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement).tagName
      if (tag === 'INPUT' || tag === 'TEXTAREA') return
      if (e.key === 'c') onCopyKey()
      if (e.key === 'd') onDeleteKey()
    }
    document.addEventListener('keydown', handler)
    return () => document.removeEventListener('keydown', handler)
  }, [open])

  // Own Escape entirely while the sheet is open: close it (same as the X button)
  // and swallow the event in the capture phase so it never reaches the history
  // page's Esc-clears-filters shortcut or the search input's own onKeyDown.
  // Radix's built-in Esc-to-close is disabled via onEscapeKeyDown on the content,
  // making this listener the single Escape owner.
  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      e.stopImmediatePropagation()
      onCloseKey()
    }
    document.addEventListener('keydown', handler, true)
    return () => document.removeEventListener('keydown', handler, true)
  }, [open])

  const content = useMemo(() => {
    if (!item) return null
    if (!item.content) {
      // Search results carry only a preview (no structured content); show it so
      // the detail body isn't blank. Copy/delete still work via `item.id`.
      return item.textPreview ? (
        <div className="whitespace-pre-wrap break-words text-[13px] leading-relaxed text-foreground/90">
          {item.textPreview}
        </div>
      ) : null
    }
    switch (item.type) {
      case 'text':
        return (
          <DetailText key={item.id} item={item.content as ClipboardTextItem} entryId={item.id} />
        )
      case 'code':
        return (
          <DetailCode key={item.id} item={item.content as ClipboardCodeItem} entryId={item.id} />
        )
      case 'link':
        return <DetailLink item={item.content as ClipboardLinkItem} />
      case 'image':
        return (
          <DetailImage key={item.id} item={item.content as ClipboardImageItem} entryId={item.id} />
        )
      case 'file':
        return (
          <DetailFile key={item.id} item={item.content as ClipboardFileItem} entryId={item.id} />
        )
      default:
        return item.textPreview ? (
          <div className="text-[13px] text-muted-foreground/70">{item.textPreview}</div>
        ) : null
    }
  }, [item])

  const CopyIcon = copyDone ? Check : Copy

  return (
    <Sheet open={open} onOpenChange={handleOpenChange}>
      <SheetContent
        side="right"
        className="w-[420px] sm:max-w-[420px] flex flex-col p-0"
        onEscapeKeyDown={e => e.preventDefault()}
      >
        {item && (
          <>
            {/* Header */}
            <SheetHeader className="shrink-0 border-b border-border/20 px-5 py-3.5">
              <SheetTitle className="flex items-center gap-2 text-sm">
                <TypeIcon className="size-4 text-muted-foreground/60" />
                {t(`history.type.${item.type}`, item.type)}
              </SheetTitle>
              <SheetDescription className="text-[11px]">
                {formatAbsoluteTime(item.activeTime)}
              </SheetDescription>
            </SheetHeader>

            {/* Content */}
            <div className="flex-1 min-h-0 overflow-y-auto px-5 py-4">{content}</div>

            {/* Meta + Delivery */}
            <div className="shrink-0 border-t border-border/20 px-5 py-3 space-y-2.5">
              {meta.length > 0 && (
                <div className="flex flex-wrap gap-x-5 gap-y-1">
                  {meta.map(m => (
                    <div key={m.label} className="flex items-center gap-1.5 text-[11px]">
                      <span className="text-muted-foreground/50">{m.label}</span>
                      <span className="text-foreground/70 font-medium">{m.value}</span>
                    </div>
                  ))}
                </div>
              )}
              {delivery && <EntryDeliveryBadge delivery={delivery} />}
            </div>

            {/* Actions */}
            <div className="shrink-0 border-t border-border/20 px-5 py-3 flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                className={copyDone ? 'flex-1 text-emerald-500 border-emerald-500/30' : 'flex-1'}
                onClick={() => void handleCopy()}
              >
                <CopyIcon className="size-3.5 mr-1.5" />
                {copyDone ? t('clipboard.item.actions.copied') : t('clipboard.item.actions.copy')}
              </Button>
              <Button
                variant="outline"
                size="sm"
                aria-label={t('clipboard.item.actions.delete')}
                className="text-destructive hover:text-destructive"
                onClick={() => void handleDelete()}
              >
                <Trash2 className="size-3.5" />
              </Button>
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  )
}

export default HistoryDetailSheet
