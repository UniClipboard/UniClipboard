import { Image as ImageIcon } from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { invalidateResourceImageDescriptor } from '@/hooks/useResourceImageDescriptor'
import { cn } from '@/lib/utils'
import {
  reportQuickPanelImageAspectRatio,
  useQuickPanelImage,
} from '@/quick-panel/hooks/useQuickPanelImage'

interface QuickPanelImageProps {
  entryId: string
  /**
   * Pre-resolved `<img src>`. Pass this when a parent has already resolved
   * the same entry via its own `useQuickPanelImage` call (e.g. `ImageGridItem`,
   * which also needs `aspectRatio`) — this component then skips its own
   * resolution entirely instead of doing redundant work for the same id.
   */
  url?: string | null
  /**
   * When false, defer this component's own resolution (daemon fetch + blob
   * decode) until true. Ignored when `url` is provided. Used by `PanelItem`'s
   * non-virtualized list to avoid eagerly fetching every image's thumbnail on
   * mount — see {@link useInView}.
   */
  enabled?: boolean
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
 * or inline image via {@link useQuickPanelImage} (unless a parent already did
 * so and passed `url` down), records the intrinsic aspect ratio on load so
 * the image wall can pack tiles without a flicker, and falls back to a
 * placeholder icon while pending or on load failure.
 */
const QuickPanelImage: React.FC<QuickPanelImageProps> = ({
  entryId,
  url: urlOverride,
  enabled = true,
  className,
  fallbackIconClassName,
  imgClassName,
}) => {
  const hasOverride = urlOverride !== undefined
  const { url: resolvedUrl } = useQuickPanelImage(entryId, { enabled: !hasOverride && enabled })
  const url = hasOverride ? urlOverride : resolvedUrl
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
            invalidateResourceImageDescriptor(entryId)
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
