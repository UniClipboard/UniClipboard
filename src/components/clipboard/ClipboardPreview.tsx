import { Clipboard, ExternalLink, File, Loader2, Image as ImageIcon } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DisplayClipboardItem } from './ClipboardContent'
import {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  fetchClipboardResourceText,
  getClipboardEntryResource,
} from '@/api/clipboardItems'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import { resolveUcUrl } from '@/lib/protocol'
import { formatFileSize } from '@/utils'

interface ClipboardPreviewProps {
  item: DisplayClipboardItem | null
}

const ClipboardPreview: React.FC<ClipboardPreviewProps> = ({ item }) => {
  const { t } = useTranslation()
  const [fullText, setFullText] = useState<string | null>(null)
  const [imageUrl, setImageUrl] = useState<string | null>(null)
  const [isLoadingText, setIsLoadingText] = useState(false)
  const [isLoadingImage, setIsLoadingImage] = useState(false)
  const [imageDimensions, setImageDimensions] = useState<{ width: number; height: number } | null>(
    null
  )

  // Load full text or image when item changes
  useEffect(() => {
    setFullText(null)
    setImageUrl(null)
    setImageDimensions(null)

    if (!item) return

    let cancelled = false

    if (item.type === 'text' || item.type === 'code') {
      const textContent = item.type === 'text' ? (item.content as ClipboardTextItem) : null
      if (textContent?.has_detail) {
        setIsLoadingText(true)
        getClipboardEntryResource(item.id)
          .then(resource => fetchClipboardResourceText(resource))
          .then(text => {
            if (!cancelled) setFullText(text)
          })
          .catch(e => console.error('Failed to load full text:', e))
          .finally(() => {
            if (!cancelled) setIsLoadingText(false)
          })
      }
    }

    if (item.type === 'image') {
      setIsLoadingImage(true)
      getClipboardEntryResource(item.id)
        .then(resource => {
          if (!cancelled) setImageUrl(resource.url)
        })
        .catch(e => console.error('Failed to load image:', e))
        .finally(() => {
          if (!cancelled) setIsLoadingImage(false)
        })
    }

    return () => {
      cancelled = true
    }
  }, [item?.id, item?.type])

  if (!item) {
    return (
      <div className="flex flex-col items-center justify-center flex-1 min-h-0 gap-3 text-muted-foreground">
        <Clipboard className="h-10 w-10 text-muted-foreground/40" />
        <span className="text-sm">{t('clipboard.preview.selectItem')}</span>
      </div>
    )
  }

  const renderContent = () => {
    switch (item.type) {
      case 'text': {
        const textItem = item.content as ClipboardTextItem
        const displayText = fullText ?? textItem.display_text
        return (
          <div className="p-4">
            {isLoadingText ? (
              <div className="flex items-center gap-2 text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                <span className="text-sm">{t('clipboard.item.loading')}</span>
              </div>
            ) : (
              <p className="whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90 break-all overflow-hidden">
                {displayText}
              </p>
            )}
          </div>
        )
      }
      case 'image': {
        const resolved = imageUrl ? resolveUcUrl(imageUrl) : null
        return (
          <div className="flex items-center justify-center p-4">
            {isLoadingImage || !resolved ? (
              <div className="flex flex-col items-center justify-center gap-2 h-48 w-full rounded-md bg-muted/30 border border-border/30">
                {isLoadingImage ? (
                  <Loader2 className="h-6 w-6 text-muted-foreground/70 animate-spin" />
                ) : (
                  <ImageIcon className="h-6 w-6 text-muted-foreground/70" />
                )}
              </div>
            ) : (
              <img
                src={resolved}
                className="max-w-full max-h-96 object-contain rounded-md"
                alt={t('clipboard.item.altText.clipboardImage')}
                onLoad={e => {
                  const img = e.currentTarget
                  if (!imageDimensions) {
                    setImageDimensions({ width: img.naturalWidth, height: img.naturalHeight })
                  }
                }}
              />
            )}
          </div>
        )
      }
      case 'link': {
        const url = (item.content as ClipboardLinkItem).url
        return (
          <div className="p-4">
            <a
              href={url}
              target="_blank"
              rel="noreferrer"
              className="text-primary font-medium hover:underline break-all text-sm leading-relaxed flex items-center gap-2"
              onClick={e => e.stopPropagation()}
            >
              <ExternalLink size={14} className="shrink-0" />
              {url}
            </a>
          </div>
        )
      }
      case 'code': {
        const code = (item.content as ClipboardCodeItem).code
        return (
          <div className="p-4">
            <div className="bg-muted/30 p-3 rounded-lg border border-border/30 overflow-auto font-mono text-xs">
              <pre className="whitespace-pre-wrap break-all text-foreground/80">
                {fullText ?? code}
              </pre>
            </div>
          </div>
        )
      }
      case 'file': {
        const fileNames = (item.content as ClipboardFileItem).file_names
        const fileSizes = (item.content as ClipboardFileItem).file_sizes
        return (
          <div className="p-4 flex flex-col gap-2">
            {fileNames.map((name, i) => (
              <div key={i} className="flex items-center gap-2 text-sm text-foreground/80">
                <File size={16} className="text-muted-foreground shrink-0" />
                <span className="truncate flex-1">{name}</span>
                {fileSizes[i] != null && (
                  <span className="text-xs text-muted-foreground">
                    {formatFileSize(fileSizes[i])}
                  </span>
                )}
              </div>
            ))}
          </div>
        )
      }
      default:
        return (
          <div className="p-4 text-muted-foreground text-sm">
            {t('clipboard.item.unknownContent')}
          </div>
        )
    }
  }

  const renderInformation = () => {
    const rows: { label: string; value: string }[] = []

    // Content type
    rows.push({
      label: t('clipboard.preview.contentType'),
      value: item.type.charAt(0).toUpperCase() + item.type.slice(1),
    })

    // Type-specific info
    if (item.type === 'text' && item.content) {
      const textItem = item.content as ClipboardTextItem
      const text = fullText ?? textItem.display_text
      rows.push({
        label: t('clipboard.preview.characters'),
        value: String(text.length),
      })
      rows.push({
        label: t('clipboard.preview.words'),
        value: String(text.split(/\s+/).filter(Boolean).length),
      })
      if (textItem.size > 0) {
        rows.push({
          label: t('clipboard.preview.size'),
          value: formatFileSize(textItem.size),
        })
      }
    }

    if (item.type === 'code' && item.content) {
      const codeItem = item.content as ClipboardCodeItem
      rows.push({
        label: t('clipboard.preview.characters'),
        value: String(codeItem.code.length),
      })
    }

    if (item.type === 'image' && item.content) {
      const imgItem = item.content as ClipboardImageItem
      const dims =
        imageDimensions ??
        (imgItem.width > 0 ? { width: imgItem.width, height: imgItem.height } : null)
      if (dims) {
        rows.push({
          label: t('clipboard.preview.dimensions'),
          value: `${dims.width} x ${dims.height}`,
        })
      }
      if (imgItem.size > 0) {
        rows.push({
          label: t('clipboard.preview.size'),
          value: formatFileSize(imgItem.size),
        })
      }
    }

    if (item.type === 'link' && item.content) {
      const url = (item.content as ClipboardLinkItem).url
      rows.push({
        label: t('clipboard.preview.characters'),
        value: String(url.length),
      })
    }

    return rows
  }

  const infoRows = renderInformation()

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {/* Content preview */}
      <ScrollArea className="flex-1 min-h-0 overflow-hidden">
        <div className="overflow-hidden">{renderContent()}</div>
      </ScrollArea>

      {/* Information section */}
      {infoRows.length > 0 && (
        <div className="shrink-0">
          <Separator className="bg-border/40" />
          <div className="p-4">
            <h4 className="text-sm font-semibold text-muted-foreground uppercase tracking-wider mb-3">
              {t('clipboard.preview.information')}
            </h4>
            <div className="grid grid-cols-2 gap-x-4 gap-y-2">
              {infoRows.map((row, i) => (
                <React.Fragment key={i}>
                  <span className="text-sm text-muted-foreground">{row.label}</span>
                  <span className="text-sm text-foreground font-medium text-right">
                    {row.value}
                  </span>
                </React.Fragment>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

export default ClipboardPreview
