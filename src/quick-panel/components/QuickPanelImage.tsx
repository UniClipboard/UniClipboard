import { Image as ImageIcon } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { cn } from '@/lib/utils'
import {
  invalidateQuickPanelImageUrl,
  reportQuickPanelImageAspectRatio,
  useQuickPanelImage,
} from '@/quick-panel/hooks/useQuickPanelImage'

interface QuickPanelImageProps {
  entryId: string
  /**
   * Wrapper class — parent controls size and positioning. The `<img>` inside
   * absolutely fills this box, so the wrapper needs `relative`.
   */
  className?: string
  /** Class for the fallback icon shown before load / on error. */
  fallbackIconClassName?: string
  /**
   * Applied only after the image successfully loads. Useful for effects that
   * shouldn't show on the placeholder frame (e.g. `object-cover` variants).
   */
  imgClassName?: string
}

/**
 * Shared image renderer for the quick panel: resolves the entry's blob-backed
 * or inline image via {@link useQuickPanelImage}, records the intrinsic aspect
 * ratio on load so the image wall can pack tiles without a flicker, and falls
 * back to a placeholder icon while pending or on load failure.
 */
const QuickPanelImage: React.FC<QuickPanelImageProps> = ({
  entryId,
  className,
  fallbackIconClassName,
  imgClassName,
}) => {
  const { url } = useQuickPanelImage(entryId)
  const [failed, setFailed] = useState(false)

  // A new URL means either a different entry (row reuse) or a retry after
  // invalidation — either way, forget the previous failure so the fresh URL
  // gets its own attempt.
  useEffect(() => {
    setFailed(false)
  }, [url])

  const showImage = url != null && !failed
  return (
    <div className={cn('relative overflow-hidden', className)}>
      {showImage ? (
        <img
          src={url}
          alt=""
          className={cn('absolute inset-0 size-full object-cover', imgClassName)}
          onLoad={event => {
            const img = event.currentTarget
            if (img.naturalWidth > 0 && img.naturalHeight > 0) {
              reportQuickPanelImageAspectRatio(entryId, img.naturalWidth / img.naturalHeight)
            }
          }}
          onError={() => {
            // Drop the descriptor cache so a subsequent mount refetches. Not
            // retried in-place: repeated 404s would burn requests on a resource
            // that's genuinely gone.
            invalidateQuickPanelImageUrl(entryId)
            setFailed(true)
          }}
        />
      ) : (
        <div className="absolute inset-0 flex items-center justify-center bg-muted/30">
          <ImageIcon className={cn('text-muted-foreground/30', fallbackIconClassName)} />
        </div>
      )}
    </div>
  )
}

export default QuickPanelImage
