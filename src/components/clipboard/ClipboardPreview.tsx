import { openUrl } from '@tauri-apps/plugin-opener'
import {
  CheckCircle2,
  Clipboard,
  CloudOff,
  ExternalLink,
  File,
  Loader2,
  Image as ImageIcon,
  Type,
  Hash,
  Database,
  Maximize,
  Files,
  Globe,
  Layers,
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
import { useAppSelector } from '@/store/hooks'
import {
  selectEntryTransferStatus,
  selectTransferByEntryId,
} from '@/store/slices/fileTransferSlice'
import { formatFileSize } from '@/utils'

/** Threshold above which we switch to virtualized rendering for performance. */
const LARGE_TEXT_THRESHOLD = 50_000

interface ClipboardPreviewProps {
  item: DisplayClipboardItem | null
  actions?: React.ReactNode
}

const ClipboardPreview: React.FC<ClipboardPreviewProps> = ({ item, actions }) => {
  const { t } = useTranslation()
  const transfer = useAppSelector(state =>
    item ? selectTransferByEntryId(state, item.id) : undefined
  )
  const entryStatus = useAppSelector(state =>
    item ? selectEntryTransferStatus(state, item.id) : undefined
  )

  const [preview, setPreview] = useState<ClipboardPreviewData | null>(null)
  const [loading, setLoading] = useState(false)
  const [imageDimensions, setImageDimensions] = useState<{ width: number; height: number } | null>(
    null
  )

  const durableStatus = entryStatus?.status
  const effectiveStatus =
    durableStatus ??
    (transfer?.status === 'active'
      ? 'transferring'
      : transfer?.status === 'failed'
        ? 'failed'
        : transfer?.status === 'completed'
          ? 'completed'
          : undefined)

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
        if (!cancelled) console.error('Failed to load clipboard preview:', e)
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
      <div className="flex flex-col items-center justify-center flex-1 min-h-0 gap-3 text-muted-foreground bg-muted/5">
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
              <p className="whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/80 break-all">
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
              <div className="flex flex-col items-center justify-center gap-2 h-64 w-full rounded-xl bg-muted/20 border border-dashed border-border/40">
                <Loader2
                  className={loading ? 'h-6 w-6 text-muted-foreground/40 animate-spin' : 'hidden'}
                />
                {!loading && <ImageIcon className="h-8 w-8 text-muted-foreground/20" />}
              </div>
            ) : (
              <img
                src={imageUrl}
                className="max-w-full max-h-[500px] object-contain rounded-lg shadow-2xl ring-1 ring-black/5 dark:ring-white/10"
                alt="Clipboard"
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
          <div className="p-8 space-y-4">
            {linkItem.urls.map((url, i) => (
              <button
                key={i}
                type="button"
                className="group flex items-center gap-3 w-full p-4 rounded-xl bg-muted/10 border border-border/20 hover:bg-muted/20 hover:border-primary/30 transition-all text-left"
                onClick={() => openUrl(url).catch(console.error)}
              >
                <div className="h-10 w-10 rounded-lg bg-primary/10 flex items-center justify-center text-primary shrink-0 group-hover:scale-110 transition-transform">
                  <ExternalLink size={18} />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="text-sm font-semibold truncate text-foreground/90">{url}</div>
                  {linkItem.domains[i] && (
                    <div className="text-xs text-muted-foreground/70 mt-0.5">
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
            <div className="relative group">
              <div className="absolute inset-0 bg-primary/5 blur-xl rounded-full opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none" />
              <pre className="relative p-5 rounded-xl bg-[#0d1117] border border-white/5 overflow-auto font-mono text-[13px] leading-relaxed text-blue-100/90 shadow-2xl">
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
          <div className="p-6 space-y-6">
            <div className="flex flex-wrap gap-2">
              {effectiveStatus === 'transferring' && (
                <div className="flex items-center gap-2 px-3 py-1 rounded-full bg-primary/10 text-primary text-xs font-bold uppercase tracking-wider">
                  <Loader2 size={12} className="animate-spin" />
                  {t('clipboard.transfer.transferring')}
                </div>
              )}
              {effectiveStatus === 'completed' && (
                <div className="flex items-center gap-2 px-3 py-1 rounded-full bg-green-500/10 text-green-500 text-xs font-bold uppercase tracking-wider">
                  <CheckCircle2 size={12} />
                  {t('clipboard.transfer.completed')}
                </div>
              )}
              {!effectiveStatus && item.isDownloaded === false && (
                <div className="flex items-center gap-2 px-3 py-1 rounded-full bg-muted/40 text-muted-foreground text-xs font-bold uppercase tracking-wider">
                  <CloudOff size={12} />
                  {t('clipboard.preview.notDownloaded')}
                </div>
              )}
            </div>
            <div className="space-y-2">
              {fileNames.map((name, i) => (
                <div
                  key={i}
                  className="flex items-center gap-4 p-3 rounded-lg bg-muted/10 border border-border/10 group hover:bg-muted/20 transition-colors"
                >
                  <div className="h-8 w-8 rounded bg-muted/20 flex items-center justify-center text-muted-foreground shrink-0 group-hover:text-primary transition-colors">
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
          <div className="p-8 text-center text-muted-foreground font-medium opacity-40 italic">
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
    <div className="flex flex-col flex-1 min-h-0 bg-background/20 backdrop-blur-sm">
      {/* 1. Inspector Header - Seamless integration via subtle surface tone */}
      {infoRows.length > 0 && (
        <div className="shrink-0 px-6 py-3 flex items-center gap-6 overflow-hidden bg-muted/10">
          {infoRows.map((row, i) => (
            <div key={i} className="flex items-center gap-2 shrink-0 group">
              <row.icon className="h-3.5 w-3.5 text-muted-foreground/20 group-hover:text-primary/50 transition-colors" />
              <span className="text-[11px] font-semibold text-muted-foreground/60 tabular-nums">
                {row.value}
              </span>
            </div>
          ))}
        </div>
      )}

      {/* 2. Scrollable Content Area - Larger padding for whitespace-driven structure */}
      <div className="flex-1 min-h-0 relative">
        {isLargeText ? (
          <div className="absolute inset-0">{renderContent()}</div>
        ) : (
          <ScrollArea className="h-full">
            <div className="min-h-full">{renderContent()}</div>
          </ScrollArea>
        )}
      </div>

      {/* 3. Global Command Footer - Separated by elevation/blur rather than lines */}
      {(effectiveStatus === 'transferring' || actions) && (
        <div className="shrink-0 px-6 py-4 bg-background/40 backdrop-blur-xl flex items-center justify-between min-h-[64px]">
          <div className="flex-1 min-w-0 mr-8">
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
