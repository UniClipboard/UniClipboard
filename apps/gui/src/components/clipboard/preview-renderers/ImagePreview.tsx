import { Image as ImageIcon, ImageDown, Loader2 } from 'lucide-react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'
import type { ClipboardImageItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { cn } from '@/lib/utils'
import { formatFileSize } from '@/utils/formatters'

interface ImagePreviewProps {
  item: ClipboardImageItem
  loading: boolean
  preview: ClipboardPreviewData | null
  setImageDimensions: (dims: { width: number; height: number } | null) => void
  /** Fit the whole image into the parent's box (no scrolling), keeping its aspect ratio. */
  fit?: boolean
  /** With `fit`: show the image at its natural size and scroll instead of scaling it down. */
  actualSize?: boolean
  /** Reports the image's MIME type once it has loaded (e.g. `image/png`). */
  onImageType?: (mime: string) => void
}

const ImagePreview: React.FC<ImagePreviewProps> = ({
  loading,
  preview,
  setImageDimensions,
  fit = false,
  actualSize = false,
  onImageType,
}) => {
  const { t } = useTranslation()
  const imageBlobPath = preview?.contentType === 'image' ? (preview.imageBlobPath ?? null) : null

  // D6 (ADR-008 P3-d): originals above the inline threshold are not auto-pulled.
  // Reveal the `<img>` (and its blob fetch) only after an explicit click, reset
  // whenever the previewed entry changes. Adjust the state during render rather
  // than in an effect, so the gate never flashes a stale frame on entry change.
  const [revealedLargeImage, setRevealedLargeImage] = useState(false)
  const [prevEntryId, setPrevEntryId] = useState(preview?.entryId)
  if (preview?.entryId !== prevEntryId) {
    setPrevEntryId(preview?.entryId)
    setRevealedLargeImage(false)
  }
  const frameClass = fit
    ? cn('flex h-full p-4', actualSize && 'overflow-auto')
    : 'flex items-center justify-center px-[var(--clipboard-preview-inset,2rem)] py-8'
  const placeholderHeight = fit ? 'h-full' : 'h-64'
  const gateLargeImage = preview?.requiresExplicitLoad === true && !revealedLargeImage

  // Resolve to a token-free `blob:` object URL; gated large images don't fetch
  // until revealed, preserving the D6 on-demand contract.
  const imageUrl = useBlobImageObjectUrl(imageBlobPath, !gateLargeImage)

  if (gateLargeImage) {
    return (
      <div className={frameClass}>
        <div
          className={`flex ${placeholderHeight} w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border/40 bg-muted/20`}
        >
          <ImageIcon className="size-8 text-muted-foreground/30" />
          <span className="text-ui-body font-medium text-foreground">
            {t('clipboard.item.largeImageTitle')}
          </span>
          <span className="text-ui-caption text-muted-foreground">
            {t('clipboard.item.largeImageHint', {
              size: formatFileSize(preview?.sizeBytes),
            })}
          </span>
          <button
            type="button"
            onClick={() => setRevealedLargeImage(true)}
            className="mt-1 inline-flex items-center gap-1.5 rounded-md border border-border/60 px-3 py-1.5 text-ui-body font-medium text-foreground transition-colors hover:bg-muted/50"
          >
            <ImageDown className="size-4" />
            {t('clipboard.item.loadLargeImage')}
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className={frameClass}>
      {loading || !imageUrl ? (
        <div
          className={`flex ${placeholderHeight} w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border/40 bg-muted/20`}
        >
          <Loader2
            className={loading ? 'size-6 animate-spin text-muted-foreground/40' : 'hidden'}
          />
          {!loading && <ImageIcon className="size-8 text-muted-foreground/20" />}
        </div>
      ) : (
        <img
          src={imageUrl}
          className={cn(
            'max-w-full rounded-lg object-contain ring-1 ring-black/5 dark:ring-white/10',
            !fit && 'max-h-[500px] shadow-2xl',
            fit && 'm-auto shadow-lg',
            fit && !actualSize && 'max-h-full',
            fit && actualSize && 'max-w-none shrink-0'
          )}
          alt={t('clipboard.item.altText.clipboardImage')}
          onLoad={event => {
            const image = event.currentTarget
            setImageDimensions({ width: image.naturalWidth, height: image.naturalHeight })
            if (onImageType) {
              // The object URL is local, so reading the type back costs no request.
              void fetch(image.currentSrc || imageUrl)
                .then(response => response.blob())
                .then(blob => blob.type && onImageType(blob.type))
                .catch(() => undefined)
            }
          }}
        />
      )}
    </div>
  )
}

export default ImagePreview
