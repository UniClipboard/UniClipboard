import { Image as ImageIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { ClipboardImageItem } from '@/lib/clipboard-entry'
import { imageTitle } from './history-card-utils'
import { useResourceImageUrl } from './useResourceImageUrl'

interface ImageEntryContentProps {
  entryId: string
  imageItem?: ClipboardImageItem | null
}

function ImageEntryContent({ entryId, imageItem }: ImageEntryContentProps) {
  const { t } = useTranslation()
  const imageUrl = useResourceImageUrl(entryId)
  const [loadedDims, setLoadedDims] = useState<{ w: number; h: number } | null>(null)
  // Reset measured dimensions when the row is reused for a different entry, so a
  // previous image's size doesn't leak into this card until onLoad fires again.
  useEffect(() => {
    setLoadedDims(null)
  }, [entryId])
  const title = imageTitle(t('history.type.image', 'image'), loadedDims, imageItem)

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

export default ImageEntryContent
