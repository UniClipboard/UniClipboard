import { Image as ImageIcon, Loader2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import type { ClipboardImageItem } from '@/api/clipboardItems'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

interface ImagePreviewProps {
  item: ClipboardImageItem
  loading: boolean
  preview: ClipboardPreviewData | null
  setImageDimensions: (dims: { width: number; height: number } | null) => void
}

const ImagePreview: React.FC<ImagePreviewProps> = ({ loading, preview, setImageDimensions }) => {
  const { t } = useTranslation()
  const imageUrl = preview?.contentType === 'image' ? (preview.imageUrl ?? null) : null

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
