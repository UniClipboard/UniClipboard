import { File, FileText, Image as ImageIcon } from 'lucide-react'
import React from 'react'
import { Filter } from '@/api/clipboardItems'
import type { ClipboardEntryType } from '@/lib/clipboard-entry'

export const PREVIEW_OPEN_DELAY_MS = 500
export const PREVIEW_SWITCH_DELAY_MS = 120

export const isMac = navigator.platform.toUpperCase().includes('MAC')

export const typeIcons: Record<ClipboardEntryType, React.ElementType> = {
  text: FileText,
  image: ImageIcon,
  richtext: FileText,
  file: File,
  unknown: FileText,
}

export function getQuickPanelLayoutClassNames(isLinux: boolean, previewOnLeft = false) {
  if (isLinux) {
    const dividerClassName = previewOnLeft ? 'border-r' : 'border-l'

    return {
      container: 'flex h-screen w-screen overflow-hidden bg-background p-0',
      statusSurface: 'bg-background',
      card: 'flex h-full w-full min-w-0 flex-col overflow-hidden border border-border/50 bg-background',
      previewCard:
        'flex h-full w-full min-w-0 flex-col overflow-hidden border border-border/50 bg-card text-card-foreground',
      previewExpanded: `flex-1 basis-0 ${dividerClassName} border-border/50 opacity-100 translate-x-0`,
      previewReserved: `shrink-0 ${dividerClassName} border-border/50 opacity-0 translate-x-0 pointer-events-none`,
    }
  }

  return {
    container: 'flex h-screen w-screen overflow-hidden bg-transparent p-4',
    statusSurface: 'bg-transparent',
    card: 'flex h-full w-full min-w-0 flex-col overflow-hidden rounded-xl border border-border/50 bg-background/95 shadow-xl backdrop-blur-xl',
    previewCard:
      'flex h-full w-full min-w-0 flex-col overflow-hidden rounded-xl border border-border/50 bg-card text-card-foreground shadow-xl backdrop-blur-xl',
    previewExpanded: 'flex-1 basis-0 opacity-100 translate-x-0',
    previewReserved: 'shrink-0 opacity-0 translate-x-0 pointer-events-none',
  }
}

/**
 * Aspect-ratio clamp for the image-wall masonry. Extreme portrait/landscape
 * thumbnails otherwise blow up column heights and make the greedy packer
 * misbehave. Shared by HistoryPane (packing) and ImageGridItem (rendering) —
 * the two MUST use the same clamp, or measured height and rendered height
 * drift out of sync.
 */
export const IMAGE_CARD_MIN_ASPECT_RATIO = 0.45
export const IMAGE_CARD_MAX_ASPECT_RATIO = 2.2

export function clampImageCardAspectRatio(aspectRatio: number | undefined): number {
  if (aspectRatio == null || !Number.isFinite(aspectRatio) || aspectRatio <= 0) return 1
  return Math.min(IMAGE_CARD_MAX_ASPECT_RATIO, Math.max(IMAGE_CARD_MIN_ASPECT_RATIO, aspectRatio))
}

/**
 * Content-type filters available in the quick panel, in display order. Single
 * source of truth for both the filter dropdown and the Tab / Shift+Tab cycle,
 * so the keyboard cycle order always matches what the menu shows. Intentionally
 * omits Filter.Favorited — it isn't surfaced in the quick panel.
 */
export const QUICK_FILTER_ORDER: Filter[] = [
  Filter.All,
  Filter.Text,
  Filter.RichText,
  Filter.Image,
  Filter.File,
]
