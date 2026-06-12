import { Code, ExternalLink, File, FileText, Image as ImageIcon } from 'lucide-react'
import React from 'react'
import { Filter } from '@/api/clipboardItems'
import type { ClipboardEntryType } from '@/lib/clipboard-entry'

export const PREVIEW_OPEN_DELAY_MS = 500
export const PREVIEW_SWITCH_DELAY_MS = 120

export const isMac = navigator.platform.toUpperCase().includes('MAC')

export const typeIcons: Record<ClipboardEntryType, React.ElementType> = {
  text: FileText,
  image: ImageIcon,
  link: ExternalLink,
  code: Code,
  file: File,
  unknown: FileText,
}

export const quickCardClassName =
  'flex h-full w-full min-w-0 flex-col overflow-hidden rounded-xl border border-border/50 bg-background/95 shadow-xl backdrop-blur-xl'

/**
 * Content-type filters available in the quick panel, in display order. Single
 * source of truth for both the filter dropdown and the Tab / Shift+Tab cycle,
 * so the keyboard cycle order always matches what the menu shows. Intentionally
 * omits Filter.Favorited — it isn't surfaced in the quick panel.
 */
export const QUICK_FILTER_ORDER: Filter[] = [
  Filter.All,
  Filter.Text,
  Filter.Image,
  Filter.Link,
  Filter.File,
  Filter.Code,
]
