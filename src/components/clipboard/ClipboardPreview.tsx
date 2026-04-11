import { openUrl } from '@tauri-apps/plugin-opener'
import {
  AlertTriangle,
  CheckCircle2,
  Clipboard,
  Clock,
  CloudOff,
  Database,
  ExternalLink,
  Files,
  File,
  Globe,
  Hash,
  Image as ImageIcon,
  Layers,
  Loader2,
  Maximize,
  Type,
} from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DisplayClipboardItem } from './ClipboardContent'
import TransferProgressBar from './TransferProgressBar'
import VirtualizedText from './VirtualizedText'
import {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
} from '@/api/clipboardItems'
import { ScrollArea } from '@/components/ui/scroll-area'
import { clipboardPreviewCache, ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { createLogger } from '@/lib/logger'
import { useAppSelector } from '@/store/hooks'
import {
  resolveEntryTransferStatus,
  selectEntryTransferStatus,
  selectTransferByTransferIds,
  selectTransferByEntryId,
} from '@/store/slices/fileTransferSlice'
import { formatFileSize } from '@/utils'

const log = createLogger('clipboard-preview')

/** Threshold above which we switch to virtualized rendering for performance. */
const LARGE_TEXT_THRESHOLD = 50_000

interface ClipboardPreviewProps {
  item: DisplayClipboardItem | null
  actions?: React.ReactNode
}

const ClipboardPreview: React.FC<ClipboardPreviewProps> = ({ item, actions }) => {
  const { t } = useTranslation()
  const transfer = useAppSelector(state =>
    item
      ? (selectTransferByEntryId(state, item.id) ??
        selectTransferByTransferIds(state, item.fileTransferIds ?? []))
      : undefined
  )
  const entryStatus = useAppSelector(state =>
    item ? selectEntryTransferStatus(state, item.id) : undefined
  )
  const effectiveStatus = resolveEntryTransferStatus(entryStatus, transfer)
  const [preview, setPreview] = useState<ClipboardPreviewData | null>(null)
  const [loading, setLoading] = useState(false)
  const [imageDimensions, setImageDimensions] = useState<{ width: number; height: number } | null>(
    null
  )

  useEffect(() => {
    setPreview(null)
    setImageDimensions(null)
    setLoading(false)

    if (!item) return

    const shouldLoadPreview =
      item.type === 'image' ||
      item.type === 'file' ||
      item.type === 'code' ||
      (item.type === 'text' && (item.content as ClipboardTextItem).has_detail)

    if (!shouldLoadPreview) return

    let cancelled = false
    setLoading(true)

    void (async () => {
      try {
        const nextPreview = await clipboardPreviewCache.get(item.id)
        if (!cancelled) setPreview(nextPreview)
      } catch (e) {
        if (!cancelled) log.error({ err: e }, 'Failed to load clipboard preview')
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()

    return () => {
      cancelled = true
    }
  }, [item])

  if (!item) {
    return (
      <div className="flex flex-1 min-h-0 flex-col items-center justify-center gap-3 bg-muted/5 text-muted-foreground">
        <Clipboard className="h-10 w-10 text-muted-foreground/20" />
        <span className="text-sm font-medium opacity-50">{t('clipboard.preview.selectItem')}</span>
      </div>
    )
  }

  const renderInformation = () => {
    const rows: { icon: React.ElementType; value: React.ReactNode }[] = []
    rows.push({
      icon: Layers,
      value: t('header.filters.' + item.type),
    })

    if (item.type === 'text' && item.content) {
      const textItem = item.content as ClipboardTextItem
      const text =
        preview?.contentType === 'text' ? (preview.textContent ?? '') : textItem.display_text
      rows.push({
        icon: Type,
        value: t('clipboard.preview.charactersCount', { count: text.length }),
      })
      if (textItem.size > 0) rows.push({ icon: Database, value: formatFileSize(textItem.size) })
    }

    if (item.type === 'code' && item.content) {
      const code =
        preview?.contentType === 'text'
          ? (preview.textContent ?? (item.content as ClipboardCodeItem).code)
          : (item.content as ClipboardCodeItem).code
      rows.push({
        icon: Type,
        value: t('clipboard.preview.charactersCount', { count: code.length }),
      })
    }

    if (item.type === 'image' && item.content) {
      const imgItem = item.content as ClipboardImageItem
      const dims =
        imageDimensions ??
        (imgItem.width > 0 ? { width: imgItem.width, height: imgItem.height } : null)
      if (dims) rows.push({ icon: Maximize, value: `${dims.width} × ${dims.height}` })
      if (imgItem.size > 0) rows.push({ icon: Database, value: formatFileSize(imgItem.size) })
    }

    if (item.type === 'file' && item.content) {
      const fileItem = item.content as ClipboardFileItem
      rows.push({
        icon: Files,
        value: t('clipboard.preview.filesCount', { count: fileItem.file_names.length }),
      })
      const knownSizes = fileItem.file_sizes.filter(s => s >= 0)
      if (knownSizes.length > 0) {
        const totalSize = knownSizes.reduce((sum, s) => sum + s, 0)
        rows.push({ icon: Database, value: formatFileSize(totalSize) })
      }
    }

    if (item.type === 'link' && item.content) {
      const linkItem = item.content as ClipboardLinkItem
      const uniqueDomains = [...new Set(linkItem.domains.filter(Boolean))]
      if (uniqueDomains.length > 0) rows.push({ icon: Globe, value: uniqueDomains[0] })
      rows.push({
        icon: Hash,
        value: t('clipboard.preview.charactersCount', { count: linkItem.urls[0]?.length ?? 0 }),
      })
    }

    return rows
  }

  const renderContent = () => {
    switch (item.type) {
      case 'text': {
        const textItem = item.content as ClipboardTextItem
        const displayText =
          preview?.contentType === 'text' ? (preview.textContent ?? '') : textItem.display_text
        if (!loading && displayText.length > LARGE_TEXT_THRESHOLD) {
          return <VirtualizedText text={displayText} className="h-full" />
        }
        return (
          <div className="p-6">
            {loading ? (
              <div className="flex items-center gap-2 text-muted-foreground/60">
                <Loader2 className="h-4 w-4 animate-spin" />
                <span className="text-sm font-medium">{t('clipboard.item.loading')}</span>
              </div>
            ) : (
              <p className="break-all whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/80">
                {displayText}
              </p>
            )}
          </div>
        )
      }
      case 'image': {
        const imageUrl = preview?.contentType === 'image' ? (preview.imageUrl ?? null) : null
        return (
          <div className="flex items-center justify-center p-8">
            {loading || !imageUrl ? (
              <div className="flex h-64 w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border/40 bg-muted/20">
                <Loader2
                  className={loading ? 'h-6 w-6 animate-spin text-muted-foreground/40' : 'hidden'}
                />
                {!loading && <ImageIcon className="h-8 w-8 text-muted-foreground/20" />}
              </div>
            ) : (
              <img
                src={imageUrl}
                className="max-h-[500px] max-w-full rounded-lg object-contain shadow-2xl ring-1 ring-black/5 dark:ring-white/10"
                alt={t('clipboard.item.altText.clipboardImage')}
                onLoad={e => {
                  const img = e.currentTarget
                  setImageDimensions({ width: img.naturalWidth, height: img.naturalHeight })
                }}
              />
            )}
          </div>
        )
      }
      case 'link': {
        const linkItem = item.content as ClipboardLinkItem
        return (
          <div className="space-y-4 p-8">
            {linkItem.urls.map((url, i) => (
              <button
                key={i}
                type="button"
                className="group flex w-full items-center gap-3 rounded-xl border border-border/20 bg-muted/10 p-4 text-left transition-all hover:border-primary/30 hover:bg-muted/20"
                onClick={() => openUrl(url).catch(err => log.error({ err }, 'Failed to open URL'))}
              >
                <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary transition-transform group-hover:scale-110">
                  <ExternalLink size={18} />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-semibold text-foreground/90">{url}</div>
                  {linkItem.domains[i] && (
                    <div className="mt-0.5 text-xs text-muted-foreground/70">
                      {linkItem.domains[i]}
                    </div>
                  )}
                </div>
              </button>
            ))}
          </div>
        )
      }
      case 'code': {
        const code =
          preview?.contentType === 'text'
            ? (preview.textContent ?? (item.content as ClipboardCodeItem).code)
            : (item.content as ClipboardCodeItem).code
        return (
          <div className="p-6">
            <div className="group relative">
              <div className="pointer-events-none absolute inset-0 rounded-full bg-primary/5 opacity-0 blur-xl transition-opacity group-hover:opacity-100" />
              <pre className="relative overflow-auto rounded-xl border border-white/5 bg-[#0d1117] p-5 font-mono text-[13px] leading-relaxed text-blue-100/90 shadow-2xl">
                <code>{code}</code>
              </pre>
            </div>
          </div>
        )
      }
      case 'file': {
        const fileNames = (item.content as ClipboardFileItem).file_names
        const fileSizes = (item.content as ClipboardFileItem).file_sizes
        return (
          <div className="space-y-6 p-6">
            <div className="flex flex-wrap gap-2">
              {effectiveStatus === 'pending' && (
                <div
                  className="flex items-center gap-2 rounded-full bg-muted/40 px-3 py-1 text-xs font-bold uppercase tracking-wider text-muted-foreground"
                  aria-label={t('clipboard.transfer.statusBadge.pending')}
                >
                  <Clock size={12} />
                  {t('clipboard.transfer.pending')}
                </div>
              )}
              {effectiveStatus === 'transferring' && (
                <div className="flex items-center gap-2 rounded-full bg-primary/10 px-3 py-1 text-xs font-bold uppercase tracking-wider text-primary">
                  <Loader2 size={12} className="animate-spin" />
                  {t('clipboard.transfer.transferring')}
                </div>
              )}
              {effectiveStatus === 'failed' && (
                <div
                  className="flex items-center gap-2 rounded-full bg-destructive/10 px-3 py-1 text-xs font-bold tracking-wider text-destructive"
                  aria-label={t('clipboard.transfer.statusBadge.failed')}
                >
                  <AlertTriangle size={12} />
                  <span>{t('clipboard.transfer.failed')}</span>
                  {entryStatus?.reason && (
                    <span className="text-destructive/70 normal-case tracking-normal">
                      {entryStatus.reason}
                    </span>
                  )}
                </div>
              )}
              {effectiveStatus === 'completed' && (
                <div className="flex items-center gap-2 rounded-full bg-green-500/10 px-3 py-1 text-xs font-bold uppercase tracking-wider text-green-500">
                  <CheckCircle2 size={12} />
                  {t('clipboard.transfer.completed')}
                </div>
              )}
              {!effectiveStatus && item.isDownloaded === false && (
                <div className="flex items-center gap-2 rounded-full bg-muted/40 px-3 py-1 text-xs font-bold uppercase tracking-wider text-muted-foreground">
                  <CloudOff size={12} />
                  {t('clipboard.preview.notDownloaded')}
                </div>
              )}
            </div>
            {effectiveStatus === 'transferring' && transfer && transfer.status === 'active' && (
              <div className="max-w-xl">
                <TransferProgressBar progress={transfer} />
              </div>
            )}
            {item.device && (
              <div className="text-xs text-muted-foreground">
                {t('clipboard.preview.sourceDevice')}: {item.device}
              </div>
            )}
            <div className="space-y-2">
              {fileNames.map((name, i) => (
                <div
                  key={i}
                  className="group flex items-center gap-4 rounded-lg border border-border/10 bg-muted/10 p-3 transition-colors hover:bg-muted/20"
                >
                  <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded bg-muted/20 text-muted-foreground transition-colors group-hover:text-primary">
                    <File size={16} />
                  </div>
                  <span className="flex-1 truncate text-sm font-medium text-foreground/80">
                    {name}
                  </span>
                  {fileSizes[i] != null && (
                    <span className="text-xs tabular-nums text-muted-foreground/60">
                      {formatFileSize(fileSizes[i])}
                    </span>
                  )}
                </div>
              ))}
            </div>
          </div>
        )
      }
      default:
        return (
          <div className="p-8 text-center font-medium italic text-muted-foreground opacity-40">
            {t('clipboard.item.unknownContent')}
          </div>
        )
    }
  }

  const infoRows = renderInformation()
  const isLargeText =
    item.type === 'text' &&
    !loading &&
    (preview?.contentType === 'text'
      ? (preview.textContent ?? '')
      : (item.content as ClipboardTextItem).display_text
    ).length > LARGE_TEXT_THRESHOLD

  return (
    <div className="flex flex-1 min-h-0 flex-col bg-background/20 backdrop-blur-sm">
      {infoRows.length > 0 && (
        <div className="shrink-0 overflow-hidden bg-muted/10 px-6 py-3">
          <div className="flex items-center gap-6">
            {infoRows.map((row, i) => (
              <div key={i} className="group flex shrink-0 items-center gap-2">
                <row.icon className="h-3.5 w-3.5 text-muted-foreground/20 transition-colors group-hover:text-primary/50" />
                <span className="text-[11px] font-semibold tabular-nums text-muted-foreground/60">
                  {row.value}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="relative flex-1 min-h-0">
        {isLargeText ? (
          <div className="absolute inset-0">{renderContent()}</div>
        ) : (
          <ScrollArea className="h-full">
            <div className="min-h-full">{renderContent()}</div>
          </ScrollArea>
        )}
      </div>

      {(effectiveStatus === 'transferring' || actions) && (
        <div className="flex min-h-[64px] shrink-0 items-center justify-between bg-background/40 px-6 py-4 backdrop-blur-xl">
          <div className="mr-8 min-w-0 flex-1">
            {effectiveStatus === 'transferring' && transfer && transfer.status === 'active' && (
              <div className="max-w-[280px]">
                <TransferProgressBar progress={transfer} variant="compact" />
              </div>
            )}
          </div>
          {actions && <div className="shrink-0">{actions}</div>}
        </div>
      )}
    </div>
  )
}

export default ClipboardPreview
