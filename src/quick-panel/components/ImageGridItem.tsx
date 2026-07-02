import React from 'react'
import { formatRelativeTime } from '@/lib/clipboard-utils'
import { cn } from '@/lib/utils'
import { clampImageCardAspectRatio, isMac } from '@/quick-panel/constants'
import { useQuickPanelImage } from '@/quick-panel/hooks/useQuickPanelImage'
import type { DisplayItem } from '@/quick-panel/types'
import QuickPanelImage from './QuickPanelImage'

interface ImageGridItemProps {
  item: DisplayItem
  index: number
  isSelected: boolean
  hoverDisabled: boolean
  onSelect: (index: number, plainOnly?: boolean) => void
  onHover: (index: number) => void
  itemRef?: React.Ref<HTMLDivElement>
  shortcutKey?: string
}

/**
 * A single tile in the quick-panel image wall. The tile subscribes to the
 * quick-panel image cache to pick up its aspect ratio (measured on first
 * `<img.onLoad>`, then cached across renders — see
 * {@link useQuickPanelImage}), so the very first paint of a previously-seen
 * entry is already the correct shape.
 */
const ImageGridItem: React.FC<ImageGridItemProps> = React.memo(
  ({ item, index, isSelected, hoverDisabled, onSelect, onHover, itemRef, shortcutKey }) => {
    const { url, aspectRatio } = useQuickPanelImage(item.id)
    const displayAspectRatio = clampImageCardAspectRatio(aspectRatio)
    const isUnavailable = item.isUnavailable

    return (
      <div
        ref={itemRef}
        role="option"
        aria-selected={isSelected}
        tabIndex={-1}
        style={{ aspectRatio: displayAspectRatio }}
        className={cn(
          'group relative mb-1 w-full break-inside-avoid cursor-pointer select-none overflow-hidden rounded-md border bg-muted/25 outline-none transition-colors',
          isSelected
            ? 'border-primary shadow-sm shadow-primary/30 ring-2 ring-primary/25'
            : hoverDisabled
              ? 'border-border/60'
              : 'border-border/60 hover:border-primary/40',
          isUnavailable && 'opacity-55'
        )}
        onClick={e => onSelect(index, e.altKey)}
        onMouseEnter={() => onHover(index)}
        onKeyDown={e => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            onSelect(index, e.altKey)
          }
        }}
      >
        <QuickPanelImage
          entryId={item.id}
          url={url}
          className="absolute inset-0 size-full"
          fallbackIconClassName="size-7"
        />

        {/* Gradient scrims so the timestamp / shortcut hint stay legible over
            bright thumbnails. Kept intentionally short (h-9) so they don't wash
            out the whole image. */}
        <div className="pointer-events-none absolute inset-x-0 top-0 h-9 bg-gradient-to-b from-black/45 to-transparent" />
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-9 bg-gradient-to-t from-black/45 to-transparent" />

        <span className="absolute right-1.5 top-1.5 text-[10px] tabular-nums text-white/80 drop-shadow">
          {formatRelativeTime(item.activeTime)}
        </span>

        {shortcutKey && (
          <kbd className="absolute bottom-1.5 right-1.5 rounded border border-white/25 bg-black/25 px-1 py-0.5 font-mono text-[9px] leading-none text-white/80 backdrop-blur">
            {isMac ? '⌘' : '⌃'}
            {shortcutKey}
          </kbd>
        )}
      </div>
    )
  }
)

export default ImageGridItem
