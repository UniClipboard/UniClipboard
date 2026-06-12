import { Image as ImageIcon, ImageDown, Loader2 } from 'lucide-react'
import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { ClipboardImageItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { formatFileSize } from '@/utils/formatters'

interface ImagePreviewProps {
  item: ClipboardImageItem
  loading: boolean
  preview: ClipboardPreviewData | null
  setImageDimensions: (dims: { width: number; height: number } | null) => void
}

const ImagePreview: React.FC<ImagePreviewProps> = ({ loading, preview, setImageDimensions }) => {
  const { t } = useTranslation()
  const imageUrl = preview?.contentType === 'image' ? (preview.imageUrl ?? null) : null

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
  const gateLargeImage = preview?.requiresExplicitLoad === true && !revealedLargeImage

  if (gateLargeImage) {
    return (
      <div className="flex items-center justify-center p-8">
        <div className="flex h-64 w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border/40 bg-muted/20">
          <ImageIcon className="size-8 text-muted-foreground/30" />
          <span className="text-sm font-medium text-foreground">
            {t('clipboard.item.largeImageTitle')}
          </span>
          <span className="text-xs text-muted-foreground">
            {t('clipboard.item.largeImageHint', {
              size: formatFileSize(preview?.sizeBytes),
            })}
          </span>
          <button
            type="button"
            onClick={() => setRevealedLargeImage(true)}
            className="mt-1 inline-flex items-center gap-1.5 rounded-md border border-border/60 px-3 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-muted/50"
          >
            <ImageDown className="size-4" />
            {t('clipboard.item.loadLargeImage')}
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex items-center justify-center p-8">
      {loading || !imageUrl ? (
        <div className="flex h-64 w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border/40 bg-muted/20">
          <Loader2
            className={loading ? 'size-6 animate-spin text-muted-foreground/40' : 'hidden'}
          />
          {!loading && <ImageIcon className="size-8 text-muted-foreground/20" />}
        </div>
      ) : (
        <img
          src={imageUrl}
          className="max-h-[500px] max-w-full rounded-lg object-contain shadow-2xl ring-1 ring-black/5 dark:ring-white/10"
          alt={t('clipboard.item.altText.clipboardImage')}
          onLoad={event => {
            const image = event.currentTarget
            setImageDimensions({ width: image.naturalWidth, height: image.naturalHeight })
          }}
        />
      )}
    </div>
  )
}

export default ImagePreview
